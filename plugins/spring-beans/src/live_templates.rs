//! Abbreviations that complete to Spring snippets with tab stops.

pub struct Template {
    pub abbreviation: &'static str,
    pub description: &'static str,
    pub snippet: &'static str,
}

const fn t(abbreviation: &'static str, description: &'static str, snippet: &'static str) -> Template {
    Template { abbreviation, description, snippet }
}

pub const JAVA: &[Template] = &[
    t("scomp", "@Component class", "@Component\npublic class ${1:Name} {\n    $0\n}"),
    t("sserv", "@Service class", "@Service\npublic class ${1:Name} {\n    $0\n}"),
    t("sconf", "@Configuration class", "@Configuration\npublic class ${1:Name} {\n    $0\n}"),
    t("sbean", "@Bean method", "@Bean\npublic ${1:Type} ${2:name}() {\n    return $0;\n}"),
    t("sauto", "@Autowired field", "@Autowired\nprivate ${1:Type} ${2:name};"),
    t("sval", "@Value field", "@Value(\"\\${${1:property.key}}\")\nprivate ${2:String} ${3:name};"),
    t("sprops", "@ConfigurationProperties class", "@ConfigurationProperties(prefix = \"${1:prefix}\")\npublic class ${2:Name}Properties {\n    $0\n}"),
    t("slisten", "@EventListener method", "@EventListener\npublic void on${1:Event}(${1:Event} event) {\n    $0\n}"),
    t("ssched", "@Scheduled method", "@Scheduled(fixedRate = ${1:5000})\npublic void ${2:run}() {\n    $0\n}"),
    t("stest", "@SpringBootTest class", "@SpringBootTest\nclass ${1:Name}Tests {\n\n    @Test\n    void ${2:contextLoads}() {\n        $0\n    }\n}"),
];

pub const KOTLIN: &[Template] = &[
    t("scomp", "@Component class", "@Component\nclass ${1:Name} {\n    $0\n}"),
    t("sserv", "@Service class", "@Service\nclass ${1:Name} {\n    $0\n}"),
    t("sbean", "@Bean function", "@Bean\nfun ${1:name}(): ${2:Type} {\n    return $0\n}"),
    t("sauto", "@Autowired property", "@Autowired\nlateinit var ${1:name}: ${2:Type}"),
    t("sval", "@Value property", "@Value(\"\\${${1:property.key}}\")\nlateinit var ${2:name}: ${3:String}"),
    t("slisten", "@EventListener function", "@EventListener\nfun on${1:Event}(event: ${1:Event}) {\n    $0\n}"),
];

pub const XML: &[Template] = &[
    t("sbean", "<bean> with class", "<bean id=\"${1:id}\" class=\"${2:com.example.Type}\">\n    $0\n</bean>"),
    t("sprop", "<property> with value", "<property name=\"${1:name}\" value=\"${2:value}\"/>"),
    t("spropref", "<property> with ref", "<property name=\"${1:name}\" ref=\"${2:bean}\"/>"),
    t("sarg", "<constructor-arg>", "<constructor-arg ref=\"${1:bean}\"/>"),
    t("simport", "<import resource>", "<import resource=\"${1:other.xml}\"/>"),
    t("sscan", "<context:component-scan>", "<context:component-scan base-package=\"${1:com.example}\"/>"),
];

pub fn for_language(language: &str) -> &'static [Template] {
    match language {
        "java" => JAVA,
        "kotlin" => KOTLIN,
        "xml" => XML,
        _ => &[],
    }
}

/// Whether `cursor` sits in element content rather than inside a tag.
pub fn outside_tag(text: &str, cursor: usize) -> bool {
    let before = &text[..cursor];
    before.rfind('<').is_none_or(|open| before[open..].contains('>'))
}
