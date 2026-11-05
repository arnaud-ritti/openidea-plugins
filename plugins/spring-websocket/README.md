# Spring WebSocket Tools

STOMP messaging support for Spring Java projects in OpenIDEA:

- **Index** of the project's broker configuration (`enableSimpleBroker`, `enableStompBrokerRelay`, `setApplicationDestinationPrefixes`, `setUserDestinationPrefix`, `addEndpoint` in a `WebSocketMessageBrokerConfigurer`), handlers (`@MessageMapping`, `@SubscribeMapping`, class-level `@MessageMapping` prefix joined) and destinations sent to (`@SendTo`, `@SendToUser`, `convertAndSend`, `convertAndSendToUser`, `send` of a messaging template).
- **Gutter arrows**: a handler shows the destination a client uses (`SEND /app/chat/send`). A destination sent to links to the other places that use it, and Go to Definition on the literal lists them all. User destinations match as the client sees them (`/user/queue/replies`).
- **Completion** in destination literals: broker prefixes and known destinations.
- **Warning** on a broker destination that no configured broker prefix covers. It only appears when the project configures a broker, and never on user destinations.

Matching is text-based, on destination literals as written; `${…}` placeholders and destinations built from variables are skipped. A `convertAndSend`/`send` call counts when its receiver is named like a messaging template (`messagingTemplate`, `simp…`), or when the destination starts with `/` and the receiver isn't a Kafka, Rabbit or JMS template.

Not ported: raw `WebSocketHandler` registration checks, SockJS options, Kotlin, client-side JavaScript STOMP code.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
