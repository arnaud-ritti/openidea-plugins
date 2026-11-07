//! Checks of a `<bean class="…">` against the project class it names, and
//! the setters of that class as `<property name>` targets. Quiet for a class
//! outside the project, and for one with a supertype or Lombok-generated
//! members the index cannot list.

use crate::model::MethodInfo;
use crate::World;
use crate::checks::Groups;
use crate::xml_checks::{dynamic, in_namespace, BEANS_NS};
use spring_text::finding::Finding;
use spring_text::xml;

fn error(from: usize, to: usize, code: &'static str, message: String) -> Finding {
    Finding::new(from, to, code, message).with_severity(openidea_plugin::Severity::Error)
}

/// The property a setter named `set_name` writes (`setURL` is `URL`, `setUserName` is `userName`).
fn property_name(setter: &str) -> Option<String> {
    let rest = setter.strip_prefix("set").filter(|r| !r.is_empty())?;
    let mut chars = rest.chars();
    let (first, second) = (chars.next()?, chars.next());
    if !first.is_uppercase() {
        return None;
    }
    let keep_upper = second.is_some_and(char::is_uppercase);
    Some(if keep_upper { rest.to_owned() } else { first.to_lowercase().chain(rest.chars().skip(1)).collect() })
}

/// The one-parameter setters of `class` and its supertypes: `(property, method, file)`.
pub fn setters<'a>(world: &World<'a>, class: &str) -> Vec<(String, &'a MethodInfo, &'a str)> {
    let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
    let Some(chain) = world.member_chain(simple, Some(&class.replace('$', "."))) else { return Vec::new() };
    let mut out = Vec::new();
    for ty in chain {
        let path = world.path_of(ty);
        for method in ty.methods.iter().filter(|m| m.params == 1 && !m.is_static) {
            if let Some(property) = property_name(&method.name) {
                out.push((property, method, path));
            }
        }
    }
    out
}

/// A `<property name>` value under the cursor: the class of its bean and the bytes of the value.
pub fn property_site(text: &str, cursor: usize) -> Option<(String, usize, usize)> {
    let els = xml::elements(text);
    let (_, e) = els.iter().enumerate().find(|(i, e)| e.local() == "property" && in_namespace(&els, *i, BEANS_NS) && e.attr("name").is_some_and(|a| a.holds(cursor)))?;
    let bean = &els[e.parent?];
    let class = bean.value("class").filter(|c| !dynamic(c) && bean.local() == "bean")?;
    let name = e.attr("name")?;
    Some((class.to_owned(), name.value_at, name.value_end))
}

/// A `<constructor-arg name>` value under the cursor: the class of its bean and the bytes of the value.
pub fn constructor_arg_site(text: &str, cursor: usize) -> Option<(String, usize, usize)> {
    let els = xml::elements(text);
    let (_, e) = els.iter().enumerate().find(|(i, e)| e.local() == "constructor-arg" && in_namespace(&els, *i, BEANS_NS) && e.attr("name").is_some_and(|a| a.holds(cursor)))?;
    let bean = &els[e.parent?];
    let class = bean.value("class").filter(|c| !dynamic(c) && bean.local() == "bean")?;
    let name = e.attr("name")?;
    Some((class.to_owned(), name.value_at, name.value_end))
}

pub fn check(els: &[xml::Element], world: &World, groups: Groups) -> Vec<Finding> {
    let mut out = Vec::new();
    if !groups.xml || !world.complete {
        return out;
    }
    let enforces_required = els.iter().any(|e| matches!(e.local(), "annotation-config" | "component-scan"));
    let default_autowire = els.first().and_then(|root| root.value("default-autowire")).is_some_and(|v| v != "no" && v != "default");
    for (i, bean) in els.iter().enumerate().filter(|(i, e)| e.local() == "bean" && in_namespace(els, *i, BEANS_NS)) {
        let Some(class) = bean.attr("class").filter(|a| !dynamic(&a.value) && bean.attrs.iter().all(|a| !dynamic(&a.value))) else { continue };
        let qualified = class.value.trim().replace('$', ".");
        let Some(ty) = world.project_type(&qualified) else { continue };
        let children: Vec<&xml::Element> = els.iter().filter(|c| c.parent == Some(i)).collect();
        let child = |name: &'static str| children.iter().filter(move |c| c.local() == name);
        let factory = bean.attr("factory-method");
        let is_abstract_bean = bean.value("abstract") == Some("true");
        let autowired = default_autowire || bean.value("autowire").is_some_and(|v| v != "no" && v != "default");
        let overridden = child("lookup-method").next().is_some() || child("replaced-method").next().is_some();

        if ty.is_abstract && factory.is_none() && bean.attr("factory-bean").is_none() && !is_abstract_bean && !overridden {
            let message = format!("{} is abstract and cannot be instantiated: use a concrete class, a factory-method, or abstract=\"true\" for a template bean.", ty.name);
            out.push(error(class.value_at, class.value_end, "xml-abstract-class", message));
        }
        if let Some(factory) = factory.filter(|f| !dynamic(&f.value) && bean.attr("factory-bean").is_none()) {
            if let Some(chain) = world.member_chain(&ty.name, Some(&qualified)) {
                let named: Vec<&MethodInfo> = chain.iter().flat_map(|t| &t.methods).filter(|m| m.name == factory.value).collect();
                let problem = if named.is_empty() {
                    Some(format!("{} has no method {}().", ty.name, factory.value))
                } else if named.iter().all(|m| !m.is_static) {
                    Some(format!("{}() is not static: a factory-method on a class needs a static method, or a factory-bean.", factory.value))
                } else {
                    None
                };
                if let Some(message) = problem {
                    out.push(error(factory.value_at, factory.value_end, "xml-factory-method", message));
                } else if named.iter().all(|m| !m.is_public) {
                    let message = format!("{}() is not public: Spring can still call it, but a public factory-method is the convention and survives tighter access settings.", factory.value);
                    out.push(Finding::new(factory.value_at, factory.value_end, "xml-factory-method-visibility", message).with_severity(openidea_plugin::Severity::Info));
                }
            }
            continue;
        }
        if bean.attr("factory-bean").is_some() {
            continue;
        }
        lookup_methods(&children, &ty.name, &qualified, world, &mut out);
        if let Some(chain) = world.member_chain(&ty.name, Some(&qualified)) {
            for attr_name in ["init-method", "destroy-method"] {
                let Some(attr) = bean.attr(attr_name).filter(|a| !dynamic(&a.value)) else { continue };
                if !chain.iter().any(|t| t.methods.iter().any(|m| m.name == attr.value)) {
                    out.push(error(attr.value_at, attr.value_end, "bean-method-missing", format!("{} has no method {}(), so Spring fails to create the bean.", ty.name, attr.value)));
                }
            }
            if enforces_required && bean.attr("parent").is_none() && !autowired && !is_abstract_bean {
                let set: Vec<&str> = child("property").filter_map(|p| p.value("name")).chain(bean.attrs.iter().filter_map(|a| a.name.strip_prefix("p:"))).collect();
                for property in chain.iter().flat_map(|t| &t.methods).filter(|m| m.required).filter_map(|m| property_name(&m.name)) {
                    if !set.iter().any(|s| s.strip_suffix("-ref").unwrap_or(s) == property) {
                        let message = format!("Property \"{property}\" is @Required in {} but this bean does not set it.", ty.name);
                        out.push(Finding::new(bean.at, bean.at + bean.name.len(), "xml-required-property", message).with_severity(openidea_plugin::Severity::Info));
                    }
                }
            }
        }
        let counted = child("constructor-arg").count();
        let c_namespace = bean.attrs.iter().any(|a| a.name.starts_with("c:"));
        if counted > 0 && !c_namespace && !autowired && !is_abstract_bean && bean.attr("parent").is_none() && ty.members_known && !ty.is_abstract {
            let accepts = |params: usize| params >= counted;
            let fits = if ty.ctors.is_empty() { accepts(0) } else { ty.ctors.iter().any(|&p| accepts(p)) };
            if !fits {
                let most = ty.ctors.iter().max().copied().unwrap_or(0);
                let message = format!("{} has no constructor taking {counted} arguments (at most {most}).", ty.name);
                out.push(error(class.value_at, class.value_end, "xml-constructor-args", message));
            }
        }
    }
    out
}

fn lookup_methods(children: &[&xml::Element], class: &str, qualified: &str, world: &World, out: &mut Vec<Finding>) {
    let Some(chain) = world.member_chain(class, Some(qualified)) else { return };
    for element in children.iter().filter(|c| matches!(c.local(), "lookup-method" | "replaced-method")) {
        let Some(name) = element.attr("name").filter(|a| !dynamic(&a.value)) else { continue };
        let named: Vec<&MethodInfo> = chain.iter().flat_map(|t| &t.methods).filter(|m| m.name == name.value).collect();
        if named.is_empty() {
            let message = format!("{class} has no method {}() to override, so Spring fails to create the bean.", name.value);
            out.push(error(name.value_at, name.value_end, "xml-lookup-method", message));
        } else if named.iter().all(|m| m.is_private || m.is_final || m.is_static) {
            let message = format!("{}() is private, final or static: the generated subclass cannot override it, so the {} has no effect.", name.value, element.local());
            out.push(Finding::new(name.value_at, name.value_end, "xml-lookup-method", message).with_severity(openidea_plugin::Severity::Info));
        }
    }
}
