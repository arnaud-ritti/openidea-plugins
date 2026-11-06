# Spring Integration Tools

Spring Integration message channels in OpenIDEA, for Java and XML configuration:

- **Channel index**: channels come from `@Bean` methods returning a `…Channel` type, `MessageChannels.direct/queue/publishSubscribe/executor/flux/priority/rendezvous("x")`, the DSL's `.channel("x")` and `.channel(c -> c.queue("x"))`, and `<int:channel id>` / `<int:publish-subscribe-channel id>` in any integration namespace.
- **Endpoints**: `@ServiceActivator`, `@Transformer`, `@Filter`, `@Router`, `@Splitter`, `@Aggregator`, `@BridgeFrom`, `@BridgeTo`, `@InboundChannelAdapter`, `@Publisher`, `@MessagingGateway`, `@Gateway` channel attributes, `IntegrationFlow.from("x")`, and the XML `input-channel`, `output-channel`, `channel`, `request-channel`, `reply-channel`, `default-request-channel`, `default-reply-channel`, `discard-channel`, `error-channel` attributes. Each is read as consuming or producing (`input-channel`, `replyChannel` and the `channel` of an outbound adapter consume; `outputChannel`, `requestChannel`, `discardChannel` produce).
- **Go to Definition** on a channel name opens its declarations, or the endpoints consuming it when nothing declares it (Spring Integration creates input channels on demand). On an XML `ref` it opens the bean (XML `<bean id>` or a Java/Kotlin bean name).
- **Completion** of known channel names, and of bean names in `ref`.
- **Gutter arrows**: a consuming endpoint points to the producers sending to its channel, a producing one to the consumers.
- **Warnings**: an output-side channel (annotation or XML) that no endpoint consumes and no bean declares; a `ref` to a bean the project doesn't declare. Input channels, DSL channels, `${…}`/`#{…}` names and common infrastructure beans (`errorChannel`, `nullChannel`, `transactionManager`…) are never reported, and nothing is reported until the first full scan of the project.

Matching is text-based, by channel name as written in string literals and attributes; constants (`outputChannel = Channels.OUT`) aren't followed. Beans and channels from libraries, auto-configuration or other modules aren't seen. Kotlin sources count for bean names and annotations; the Kotlin DSL doesn't.

- **Integration Channels** tool window (bottom): message channels as nodes (declared ones accented) and each endpoint (service activator, transformer, filter, router, gateway, adapter…) as a small node, drawn input channel -> endpoint -> output channel from the project index; click a node to open its source. It refreshes on open and with its Refresh button, which rescans.

Not ported: the DSL flow chain edges in the diagram (`.from("a").channel("b")` shows both channels but no endpoint node), adapter-specific attribute checks (JMS, AMQP, HTTP…), the Kotlin DSL, SpEL.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
