//! What a Blade view says about the others: the layout it extends, the views
//! it includes, and its sections, yields, pushes and stacks.

use crate::php;
use crate::refs::blade_regions;

/// A directive argument and where its contents start.
pub type Named = (String, usize);

#[derive(Default, Debug)]
pub struct Facts {
    pub extends: Option<String>,
    pub includes: Vec<String>,
    pub sections: Vec<Named>,
    pub yields: Vec<Named>,
    pub pushes: Vec<Named>,
    pub stacks: Vec<Named>,
}

pub fn facts(text: &str) -> Facts {
    let mut facts = Facts::default();

    for region in blade_regions(text) {
        let Some(directive) = text[region.clone()].strip_prefix('@') else { continue };
        let name_length = directive.bytes().position(|b| !b.is_ascii_alphabetic()).unwrap_or(directive.len());
        let name = &directive[..name_length];
        let Some(open) = text[region.start..region.end].find('(').map(|i| region.start + i) else { continue };

        let arguments = php::arguments(text, open);
        let argument = |i: usize| arguments.get(i).and_then(|r| php::literal(text, r.clone())).map(|(v, at)| (v.to_owned(), at));

        match name {
            "extends" => facts.extends = argument(0).map(|(v, _)| v),
            "include" | "includeIf" | "component" => facts.includes.extend(argument(0).map(|(v, _)| v)),
            "includeWhen" | "includeUnless" => facts.includes.extend(argument(1).map(|(v, _)| v)),
            "each" => facts.includes.extend([argument(0), argument(3)].into_iter().flatten().map(|(v, _)| v)),
            "section" => facts.sections.extend(argument(0)),
            "yield" => facts.yields.extend(argument(0)),
            "push" | "pushOnce" | "prepend" | "prependOnce" => facts.pushes.extend(argument(0)),
            "stack" => facts.stacks.extend(argument(0)),
            _ => {}
        }
    }

    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_layout_sections_and_stacks() {
        let text = "@extends('layouts.app')\n@section('content')\n@include('partials.nav')\n@each('row', $rows, 'r', 'empty')\n@push('scripts')\n@endpush\n@stack('styles')\n@yield('title')\n@section('title', 'Home')";
        let facts = facts(text);
        assert_eq!(facts.extends.as_deref(), Some("layouts.app"));
        assert_eq!(facts.includes, ["partials.nav", "row", "empty"]);
        let names = |named: &[Named]| named.iter().map(|(n, at)| (n.clone(), text[*at..].starts_with(n.as_str()))).collect::<Vec<_>>();
        assert_eq!(names(&facts.sections), [("content".into(), true), ("title".into(), true)]);
        assert_eq!(names(&facts.pushes), [("scripts".into(), true)]);
        assert_eq!(names(&facts.stacks), [("styles".into(), true)]);
        assert_eq!(names(&facts.yields), [("title".into(), true)]);
    }
}
