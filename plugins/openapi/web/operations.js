// The operations of the spec this editor group shows, grouped by tag. The plugin's code
// parses the file (the endpoints view's model) and moves the caret; the page only filters.
const frame = Math.random().toString(36).slice(2);
const filter = document.getElementById("filter");
const list = document.getElementById("list");
const status = document.getElementById("status");
const editor = window.openidea.editor;
let target = null;
let reply = null;
let asked = -1;
let timer = 0;
// API 1.10 editor data, absent in older IDEs: the main caret's line and the marker bar's problems.
let caret = null;
let problems = [];

function ask(next) {
  if (!next?.path) {
    target = reply = null;
    return show();
  }
  if (next.path !== target?.path) reply = null;
  else if (next.version === asked) return;
  target = next;
  asked = next.version;
  window.openidea.postMessage(JSON.stringify({ op: "outline", id: frame, path: next.path }));
  show();
}

function say(text) {
  status.textContent = "";
  const empty = document.createElement("p");
  empty.className = "ui-empty";
  empty.textContent = text;
  list.replaceChildren(empty);
}

function row(operation) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ops-row";
  const method = document.createElement("span");
  method.className = "ui-http-method";
  method.dataset.method = operation.method;
  method.textContent = operation.method;
  const path = document.createElement("span");
  path.className = "ops-path";
  path.textContent = operation.path;
  button.append(method, path);
  button.operation = operation;
  button.addEventListener("click", () =>
    window.openidea.postMessage(JSON.stringify({ op: "open", path: reply.path, line: operation.line, column: operation.column })),
  );
  return button;
}

function show() {
  if (!target) return say("This editor shows no file.");
  if (!reply) return say("Reading operations…");
  if (reply.message) return say(reply.message);
  const all = reply.operations;
  if (!all.length) return say("This spec has no operations yet.");
  const words = filter.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const shown = all.filter((o) => {
    const text = `${o.method} ${o.path} ${o.tag} ${o.operationId ?? ""}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
  if (!shown.length) return say(`No operation matches “${filter.value.trim()}”. Press Escape to clear the filter.`);
  const groups = new Map();
  for (const o of shown) groups.set(o.tag, [...(groups.get(o.tag) ?? []), o]);
  list.replaceChildren(
    ...[...groups].map(([tag, members]) => {
      const section = document.createElement("section");
      const heading = document.createElement("h2");
      heading.className = "ops-tag ui-group-header";
      const count = document.createElement("span");
      count.textContent = members.length;
      heading.append(tag, count);
      section.setAttribute("aria-label", tag);
      section.append(heading, ...members.map(row));
      return section;
    }),
  );
  status.textContent = shown.length === all.length ? `${all.length} operations` : `${shown.length} of ${all.length} operations`;
  mark();
}

// The worst problem inside an operation's lines, or null.
function severity(operation) {
  const last = operation.endLine ?? operation.line;
  let found = null;
  for (const problem of problems) {
    if (problem.line > last || problem.lastLine < operation.line) continue;
    if (problem.severity === "error") return "error";
    if (problem.severity === "warning") found = "warning";
  }
  return found;
}

// Tints the operation holding the caret and flags those with errors or warnings.
function mark() {
  for (const button of list.querySelectorAll(".ops-row")) {
    const operation = button.operation;
    const current = caret !== null && caret >= operation.line && caret <= (operation.endLine ?? operation.line);
    if (current && !button.classList.contains("is-current") && !list.contains(document.activeElement)) {
      button.scrollIntoView({ block: "nearest" });
    }
    button.classList.toggle("is-current", current);
    if (current) button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
    const problem = severity(operation);
    if (problem) button.dataset.problem = problem;
    else delete button.dataset.problem;
    const note = problem === "error" ? " · has errors" : problem === "warning" ? " · has warnings" : "";
    button.title = `${operation.method} ${operation.path}${operation.operationId ? ` (${operation.operationId})` : ""}${note}`;
  }
}

function onSelections(selections) {
  caret = selections?.[0]?.head?.line ?? null;
  mark();
}

function onProblems(found) {
  problems = found ?? [];
  mark();
}

window.openidea.onMessage((text) => {
  const message = JSON.parse(text);
  // Replies go to the panel of every editor group.
  if (message.id !== frame || message.path !== target?.path) return;
  reply = message;
  show();
});

filter.addEventListener("input", show);
filter.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && filter.value) {
    filter.value = "";
    show();
  } else if (e.key === "ArrowDown") {
    list.querySelector(".ops-row")?.focus();
    e.preventDefault();
  }
});
list.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const rows = [...list.querySelectorAll(".ops-row")];
  const at = rows.indexOf(document.activeElement);
  const next = e.key === "ArrowDown" ? rows[at + 1] : at > 0 ? rows[at - 1] : filter;
  next?.focus();
  e.preventDefault();
});

editor.get().then(ask);
editor.onTarget(ask);
if (editor.selections) {
  Promise.resolve(editor.selections()).then(onSelections);
  editor.onSelections(onSelections);
}
if (editor.problems) {
  Promise.resolve(editor.problems()).then(onProblems);
  editor.onProblems(onProblems);
}
// Edits re-ask after a pause, and only when the version moved: highlight changes come without
// the document's text, which `document.onChange` would copy on every keystroke.
editor.onHighlights(() => {
  clearTimeout(timer);
  timer = setTimeout(() => editor.get().then(ask), 600);
});
