// Shows the custom editor's document in Swagger UI and follows its edits.
// The plugin's code sends the files the document references ({ files: { "./a.yaml": text } });
// the frame has no network, so fetches of those URLs are answered from them.
const root = "https://openapi.preview/spec/";
let latest = "";
let files = new Map();
const nativeFetch = window.fetch.bind(window);
let mode = "swagger";
let redocLoaded;
window.fetch = (input, init) => {
  const url = new URL(typeof input === "string" ? input : input.url, root).href.split("#")[0];
  if (mode === "redoc" && url === `${root}openapi`) return Promise.resolve(new Response(latest));
  return files.has(url) ? Promise.resolve(new Response(files.get(url))) : nativeFetch(input, init);
};
const ui = SwaggerUIBundle({
  dom_id: "#swagger",
  supportedSubmitMethods: [], // the frame has no network: no "Try it out"
  validatorUrl: null,
  deepLinking: false,
});
const loadRedoc = () => {
  redocLoaded ??= new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "./vendor/redoc.standalone.js";
    script.onload = resolve;
    script.onerror = () => reject(new Error("Redoc failed to load"));
    document.head.appendChild(script);
  });
  return redocLoaded;
};
const showRedoc = () => {
  const stale = document.getElementById("redoc");
  const fresh = document.createElement("div");
  fresh.id = "redoc";
  fresh.style.display = "block";
  stale.replaceWith(fresh);
  loadRedoc()
    .then(() => Redoc.init(`${root}openapi`, { disableSearch: true, hideDownloadButton: true }, fresh)) // search needs a worker; the frame has no network
    .catch((error) => {
      fresh.textContent = String(error);
    });
};
const show = (text) => {
  if (mode === "redoc") return showRedoc();
  ui.specActions.updateUrl(`${root}openapi`);
  ui.specActions.updateSpec(text); // parses YAML and JSON
};
let timer = 0;
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
document.getElementById("reload").addEventListener("click", () => {
  known = "";
  window.openidea.document.get().then((doc) => render(doc.text));
});
document.getElementById("switch").addEventListener("click", (event) => {
  mode = mode === "swagger" ? "redoc" : "swagger";
  event.target.textContent = mode === "swagger" ? "Redoc" : "Swagger UI";
  document.getElementById("swagger").style.display = mode === "swagger" ? "" : "none";
  document.getElementById("redoc").style.display = mode === "redoc" ? "block" : "none";
  show(latest);
});
