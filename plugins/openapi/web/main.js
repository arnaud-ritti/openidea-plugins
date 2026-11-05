// Shows the custom editor's document in Swagger UI and follows its edits.
const ui = SwaggerUIBundle({
  dom_id: "#swagger",
  supportedSubmitMethods: [], // the frame has no network: no "Try it out"
  validatorUrl: null,
  deepLinking: false,
});
const show = (text) => ui.specActions.updateSpec(text); // parses YAML and JSON
let timer = 0;
window.openidea.document.onChange((state) => {
  clearTimeout(timer);
  timer = setTimeout(() => show(state.text), 300);
});
window.openidea.document.get().then((doc) => show(doc.text));
