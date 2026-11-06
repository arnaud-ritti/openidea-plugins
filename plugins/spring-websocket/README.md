# Spring WebSocket Tools

STOMP messaging support for Spring Java and Kotlin projects in OpenIDEA:

- **Index** of the project's broker configuration (`enableSimpleBroker`, `enableStompBrokerRelay`, `setApplicationDestinationPrefixes`, `setUserDestinationPrefix`, `addEndpoint` in a `WebSocketMessageBrokerConfigurer`), handlers (`@MessageMapping`, `@SubscribeMapping`, class-level `@MessageMapping` prefix joined) and destinations sent to (`@SendTo`, `@SendToUser`, `convertAndSend`, `convertAndSendToUser`, `send` of a messaging template).
- **Gutter arrows**: a handler shows the destination a client uses (`SEND /app/chat/send`). A destination sent to links to the other places that use it, and Go to Definition on the literal lists them all. User destinations match as the client sees them (`/user/queue/replies`).
- **Completion** in destination literals: broker prefixes and known destinations.
- **Warning** on a broker destination that no configured broker prefix covers. It only appears when the project configures a broker, and never on user destinations.

Matching is text-based, on destination literals as written; `${…}` placeholders and destinations built from variables are skipped. A `convertAndSend`/`send` call counts when its receiver is named like a messaging template (`messagingTemplate`, `simp…`), or when the destination starts with `/` and the receiver isn't a Kafka, Rabbit or JMS template.

- **JavaScript/TypeScript STOMP clients** (`.js`, `.jsx`, `.ts`, `.tsx`, `.vue`, outside `node_modules`, `dist`, `build`): `client.subscribe('/topic/x', …)` links to the matching `@SendTo`/`convertAndSend`/`@SubscribeMapping`, and `client.publish({ destination: '/app/x' })` or `client.send('/app/x', …)` links to the `@MessageMapping` handler (application prefix applied). The Java side lists the client usages in its gutter tooltip. A call counts when its receiver name contains `stomp`, or was assigned `new Client(`, `new StompJs.Client(`, `Stomp.over(` or `Stomp.client(` in the same file (`httpClient.send` and the like are ignored). Only plain string literals starting with `/` count; template strings with `${…}` and concatenations are skipped. One target opens per marker; the tooltip lists the rest.

  JS/TS/Vue documents are only read and linked in a project whose root `pom.xml`, `build.gradle` or `build.gradle.kts` mentions `spring-boot-starter-websocket`, `spring-messaging` or `spring-websocket`, or once a Java or Kotlin file of the project has been opened in the session; elsewhere they are left alone (no scan of the project). A backend in a sub-module with no such root build file is picked up only after one of its Java/Kotlin files is opened.

- **Spring XML** (`http://www.springframework.org/schema/websocket`): required attributes of `stomp-endpoint` (`path`), `mapping` (`path`, `handler`), `simple-broker` and `stomp-broker-relay` (`prefix`) and `handshake-handler` (`ref`); a `handler`/`ref` that names no bean of the project is an Info (beans of libraries are invisible; quiet until the whole project was read). The tables are hard-coded since no schema is at hand. Unverified in the IDE: the `xml` provider activation.

Not ported: raw `WebSocketHandler` registration checks, SockJS options.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
