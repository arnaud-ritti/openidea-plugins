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
