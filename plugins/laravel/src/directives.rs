//! Blade directive names: Laravel's and Livewire's own, and the position of
//! the name being typed after an `@`.

/// The directives `aritti.blade` knows; a test keeps the two lists equal.
// ponytail: the plugins are separate Cargo projects, so the list is copied, not shared.
pub const CORE: &[&str] = &[
    "if", "endif", "hasSection", "sectionMissing", "unless", "endunless", "isset", "endisset", "empty", "endempty",
    "auth", "endauth", "guest", "endguest", "production", "endproduction", "env", "endenv", "section", "endsection",
    "show", "stop", "append", "overwrite", "push", "endpush", "pushIf", "endPushIf", "prepend", "endprepend",
    "pushOnce", "endPushOnce", "prependOnce", "endPrependOnce", "once", "endonce", "for", "endfor", "foreach",
    "endforeach", "forelse", "endforelse", "while", "endwhile", "switch", "endswitch", "php", "endphp", "verbatim",
    "endverbatim", "error", "enderror", "component", "endcomponent", "slot", "endslot", "can", "endcan", "cannot",
    "endcannot", "canany", "endcanany", "session", "endsession", "fragment", "endfragment", "persist", "endpersist",
    "teleport", "endteleport", "script", "endscript", "assets", "endassets", "else", "elseif", "elsecan",
    "elsecannot", "elsecanany", "extends", "yield", "parent", "include", "includeIf", "includeWhen", "includeUnless",
    "includeFirst", "each", "stack", "continue", "break", "case", "default", "props", "aware", "class", "style",
    "checked", "selected", "disabled", "readonly", "required", "csrf", "method", "json", "js", "lang", "choice",
    "use", "inject", "vite", "viteReactRefresh", "dd", "dump", "livewire", "livewireStyles", "livewireScripts",
    "livewireScriptConfig", "bool", "unlessenv",
];

/// `(directive, insert text)` in snippet syntax, the `@` excluded, a literal `$` written `\$` in snippet syntax.
pub const SNIPPETS: &[(&str, &str)] = &[
    ("if", "if(${1:condition})\n    $0\n@endif"),
    ("unless", "unless(${1:condition})\n    $0\n@endunless"),
    ("isset", "isset(${1:\\$variable})\n    $0\n@endisset"),
    ("empty", "empty(${1:\\$variable})\n    $0\n@endempty"),
    ("auth", "auth\n    $0\n@endauth"),
    ("guest", "guest\n    $0\n@endguest"),
    ("env", "env('${1:local}')\n    $0\n@endenv"),
    ("production", "production\n    $0\n@endproduction"),
    ("section", "section('${1:name}')\n    $0\n@endsection"),
    ("push", "push('${1:stack}')\n    $0\n@endpush"),
    ("pushOnce", "pushOnce('${1:stack}')\n    $0\n@endPushOnce"),
    ("prepend", "prepend('${1:stack}')\n    $0\n@endprepend"),
    ("for", "for(${1:\\$i = 0}; ${2:\\$i < 10}; ${3:\\$i++})\n    $0\n@endfor"),
    ("foreach", "foreach(${1:\\$items} as ${2:\\$item})\n    $0\n@endforeach"),
    ("forelse", "forelse(${1:\\$items} as ${2:\\$item})\n    $0\n@empty\n    ${3:empty}\n@endforelse"),
    ("while", "while(${1:condition})\n    $0\n@endwhile"),
    ("switch", "switch(${1:\\$value})\n    $0\n@endswitch"),
    ("can", "can('${1:ability}')\n    $0\n@endcan"),
    ("cannot", "cannot('${1:ability}')\n    $0\n@endcannot"),
    ("canany", "canany(['${1:ability}'])\n    $0\n@endcanany"),
    ("error", "error('${1:field}')\n    $0\n@enderror"),
    ("component", "component('${1:name}')\n    $0\n@endcomponent"),
    ("slot", "slot('${1:name}')\n    $0\n@endslot"),
    ("php", "php\n    $0\n@endphp"),
    ("verbatim", "verbatim\n    $0\n@endverbatim"),
    ("extends", "extends('${1:layout}')"),
    ("yield", "yield('${1:section}')"),
    ("include", "include('${1:view}')"),
    ("includeIf", "includeIf('${1:view}')"),
    ("includeWhen", "includeWhen(${1:condition}, '${2:view}')"),
    ("includeUnless", "includeUnless(${1:condition}, '${2:view}')"),
    ("includeFirst", "includeFirst(['${1:first}', '${2:second}'])"),
    ("each", "each('${1:view}', ${2:\\$items}, '${3:item}')"),
    ("stack", "stack('${1:stack}')"),
    ("props", "props(['${1:name}'])"),
    ("class", "class(['${1:class}'])"),
    ("style", "style(['${1:style}'])"),
    ("method", "method('${1:PUT}')"),
    ("json", "json(${1:\\$data})"),
    ("vite", "vite(['${1:resources/css/app.css}', '${2:resources/js/app.js}'])"),
    ("lang", "lang('${1:key}')"),
    ("inject", "inject('${1:name}', '${2:Service}')"),
    ("livewire", "livewire('${1:component}')"),
    ("csrf", "csrf"),
];

/// Byte range of the directive name being typed, the `@` excluded, when the
/// cursor sits right after `@name`.
pub fn name_at(text: &str, offset: usize) -> Option<(usize, usize)> {
    let before = text.get(..offset)?;
    let name_length = before.bytes().rev().take_while(u8::is_ascii_alphabetic).count();
    let start = offset - name_length;
    let at = start.checked_sub(1).filter(|i| text.as_bytes()[*i] == b'@')?;

    let previous = text[..at].chars().next_back();
    if previous.is_some_and(|c| c == '@' || c == '_' || c == '.' || c.is_alphanumeric()) {
        return None;
    }

    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..at];
    let open = ["{{", "{!!"].iter().filter_map(|o| line.rfind(o)).max();
    let close = ["}}", "!!}"].iter().filter_map(|c| line.rfind(c)).max();
    if open.is_some_and(|o| close.is_none_or(|c| c < o)) {
        return None;
    }

    let end = offset + text[offset..].bytes().take_while(u8::is_ascii_alphabetic).count();
    Some((start, end))
}
