# Spring Web Flow Tools

Spring Web Flow 2 and 3 XML flow definitions in OpenIDEA (files whose root is `<flow>` in the `http://www.springframework.org/schema/webflow` namespace):

- **States**: Go to Definition and completion on `to` of `<transition>`, `then` and `else` of `<if>` in a `<decision-state>`, and `start-state` of `<flow>`. They lead to the state of the same flow, then to a state inherited from a parent flow.
- **Flows**: `subflow` of `<subflow-state>` and `parent` of `<flow>` (comma-separated) open the flow file and complete flow ids; `parent="flowId#stateId"` on a state opens the parent flow or its state, and completes that flow's states. A flow's id is its file name without `.xml`. A flow registry in the project's XML (`webflow-config` namespace) adds ids: `<flow-location id path>` names the file at that path, and `<flow-registry base-path>` names a flow in a subfolder after that folder (`/WEB-INF/flows/booking/booking-flow.xml` is `booking`).
- **Checks**: a transition, `then` or `else` naming no state of the flow or its parents is an error, so is a `start-state` naming no state and a state id used twice. A `subflow` naming no known flow is a warning, once the whole project has been read. A flow without an `<end-state>` gets a note, unless it is abstract or has a parent. Expressions (`${…}`, `#{…}`) are never reported, and state checks are skipped while a parent flow can't be found.
- **Web Flow tool window** (right side): the flow of the active editor as a diagram. Each state shows its id and kind, the start state is highlighted, and transitions are arrows labelled with their `on` event. A decision's arrows show the `test` of each `<if>` (cut short, full text on hover) and `else`. It follows the focused editor and the edits to the flow; Refresh redraws it. Global transitions aren't drawn.

Matching is text-based: ids as written in attributes. Flows registered in Java configuration (`FlowDefinitionRegistryBuilder`), in libraries or in other modules aren't seen.

Not covered: Java flow registry configuration, view and bean references (`view`, `expression`, `evaluate`), flow input/output mappings, SpEL in expressions, and clicking diagram nodes (the desktop IDE blocks page scripts).

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
