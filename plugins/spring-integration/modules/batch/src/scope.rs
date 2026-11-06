use crate::NAMESPACE;
use openidea_plugin::{Diagnostic, Severity};
use spring_text::java::{self, Annotation, MemberKind, Str, TypeDecl};
use spring_text::{range, xml};

const LATE_BINDING: [&str; 4] = ["jobParameters", "jobExecutionContext", "stepExecutionContext", "stepExecution"];
const SCOPES: [&str; 3] = ["StepScope", "JobScope", "Scope"];
const COMPONENTS: [&str; 3] = ["Component", "Service", "Repository"];

fn late_bound(s: &Str) -> bool {
    s.value.contains("#{") && LATE_BINDING.iter().any(|name| s.value.contains(&format!("{name}[")) || s.value.contains(&format!("{name}.")))
}

fn scoped(annotations: &[Annotation]) -> bool {
    annotations.iter().any(|a| SCOPES.contains(&a.name.as_str()))
}

fn value_strings(annotations: &[Annotation]) -> impl Iterator<Item = &Str> {
    annotations.iter().filter(|a| a.name == "Value").flat_map(|a| a.args.iter().flat_map(|arg| arg.strings.iter()))
}

fn report(text: &str, s: &Str) -> Diagnostic {
    Diagnostic::new(
        range(text, s.at, s.end),
        Severity::Error,
        "Late binding of job or step values only works in a bean with step or job scope; add @StepScope or @JobScope, or the expression fails when the bean is created.",
    )
    .code("late-binding-without-scope")
}

fn in_scoped_type(ty: &TypeDecl) -> bool {
    scoped(&ty.annotations)
}

/// `#{jobParameters[...]}` in `@Value` of a `@Bean` method parameter or of a `@Component`'s field or constructor parameter whose bean has no step or job scope.
pub fn java_problems(text: &str) -> Vec<Diagnostic> {
    if !text.contains("#{") {
        return Vec::new();
    }
    let file = java::parse(text);
    if !(file.from_spring("Value") && file.from_spring("Bean") && SCOPES.iter().all(|s| file.from_spring(s))) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for ty in file.types.iter().filter(|t| !in_scoped_type(t)) {
        let component = ty.annotations.iter().any(|a| COMPONENTS.contains(&a.name.as_str()));
        for m in &ty.members {
            let flagged: Vec<&Str> = match m.kind {
                MemberKind::Method if m.annotation("Bean").is_some() && !scoped(&m.annotations) => {
                    m.params.iter().flat_map(|p| value_strings(&p.annotations)).collect()
                }
                MemberKind::Constructor if component => m.params.iter().flat_map(|p| value_strings(&p.annotations)).collect(),
                MemberKind::Field if component => value_strings(&m.annotations).collect(),
                _ => continue,
            };
            out.extend(flagged.into_iter().filter(|s| late_bound(s)).map(|s| report(text, s)));
        }
    }
    out
}

/// `#{jobParameters[...]}` in an attribute of a plain `<bean>` (or inside one) without `scope="step"` or `scope="job"`.
pub fn xml_problems(text: &str) -> Vec<Diagnostic> {
    if !text.contains("#{") || !LATE_BINDING.iter().any(|n| text.contains(n)) {
        return Vec::new();
    }
    let els = xml::elements(text);
    let mut out = Vec::new();
    for (index, e) in els.iter().enumerate() {
        for attr in e.attrs.iter().filter(|a| a.value.contains("#{") && LATE_BINDING.iter().any(|n| a.value.contains(&format!("{n}[")))) {
            let mut beans = Vec::new();
            let mut at = Some(index);
            while let Some(i) = at {
                if els[i].local() == "bean" {
                    beans.push(&els[i]);
                }
                at = els[i].parent;
            }
            if beans.is_empty() || xml::namespace(&els, index) == Some(NAMESPACE) {
                continue;
            }
            let step_scoped = beans.iter().any(|b| b.value("scope").is_some_and(|s| s.starts_with("${") || s == "step" || s == "job"));
            if !step_scoped {
                out.push(
                    Diagnostic::new(
                        range(text, attr.value_at, attr.value_end),
                        Severity::Error,
                        "Late binding of job or step values only works in a bean with scope=\"step\" or scope=\"job\".",
                    )
                    .code("late-binding-without-scope"),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r##"import org.springframework.batch.core.configuration.annotation.StepScope;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.context.annotation.Bean;
class C {
    @Bean
    ItemReader<X> bad(@Value("#{jobParameters['file']}") String file) { return null; }
    @Bean @StepScope
    ItemReader<X> good(@Value("#{jobParameters['file']}") String file) { return null; }
    @Bean
    ItemReader<X> plain(@Value("${file}") String file, @Value("#{systemProperties['x']}") String x) { return null; }
}
@Component
class S {
    @Value("#{stepExecutionContext['n']}") int n;
}
@Component @StepScope
class T {
    @Value("#{jobParameters['x']}") String x;
}
"##;

    #[test]
    fn flags_late_binding_without_scope() {
        let found: Vec<_> = java_problems(CONFIG).iter().map(|d| (d.range.start.line, d.code.clone().unwrap())).collect();
        assert_eq!(found, [(5, "late-binding-without-scope".to_owned()), (13, "late-binding-without-scope".to_owned())]);
    }

    #[test]
    fn is_quiet_for_a_foreign_scope_annotation() {
        let foreign = CONFIG.replace("org.springframework.batch.core.configuration.annotation.StepScope", "com.acme.StepScope");
        assert!(java_problems(&foreign).is_empty());
    }

    #[test]
    fn flags_unscoped_xml_beans() {
        let xml = r##"<beans xmlns="http://www.springframework.org/schema/beans">
<bean id="a" class="R"><property name="f" value="#{jobParameters['f']}"/></bean>
<bean id="b" class="R" scope="step"><property name="f" value="#{jobParameters['f']}"/></bean>
<bean id="c" class="R" scope="step"><property name="h"><bean class="H"><property name="f" value="#{jobParameters['f']}"/></bean></property></bean>
<bean id="d" class="R"><property name="f" value="#{systemProperties['f']}"/></bean>
</beans>"##;
        let found: Vec<_> = xml_problems(xml).iter().map(|d| d.range.start.line).collect();
        assert_eq!(found, [1]);
    }
}
