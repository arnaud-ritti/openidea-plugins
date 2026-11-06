// Shows the custom editor's document in Swagger UI and follows its edits.
// The plugin's code sends the files the document references ({ files: { "./a.yaml": text } });
// the frame has no network, so fetches of those URLs are answered from them.
const root = "https://openapi.preview/spec/";
let files = new Map();
const nativeFetch = window.fetch.bind(window);
window.fetch = (input, init) => {
  const url = new URL(typeof input === "string" ? input : input.url, root).href.split("#")[0];
  return files.has(url) ? Promise.resolve(new Response(files.get(url))) : nativeFetch(input, init);
};
const ui = SwaggerUIBundle({
  dom_id: "#swagger",
  supportedSubmitMethods: [], // the frame has no network: no "Try it out"
  validatorUrl: null,
  deepLinking: false,
});
const show = (text) => {
  ui.specActions.updateUrl(`${root}openapi`);
  ui.specActions.updateSpec(text); // parses YAML and JSON
};
let timer = 0;
let latest = "";
const id = globalThis.crypto?.getRandomValues
  ? Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => b.toString(16).padStart(2, "0")).join("")
  : Math.random().toString(36).slice(2);
let known = "";
const render = (text) => {
  latest = text;
  window.openidea.postMessage(`files\t${id}\t${known}`);
  show(text);
};
window.openidea.onMessage((message) => {
  const reply = JSON.parse(message);
  if (reply.id !== id) return; // replies go to every open preview
  known = reply.hash;
  const received = reply.files ?? {};
  files = new Map(Object.entries(received).map(([name, text]) => [new URL(name, root).href.split("#")[0], text]));
  show(latest);
});
window.openidea.document.onChange((state) => {
  clearTimeout(timer);
  timer = setTimeout(() => render(state.text), 300);
});
window.openidea.document.get().then((doc) => render(doc.text));
