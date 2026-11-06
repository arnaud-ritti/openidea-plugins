# Spring Messaging Tools

Producer ↔ consumer navigation for Spring Kafka, Spring AMQP (RabbitMQ) and Spring JMS (ActiveMQ/Artemis) in Java and Kotlin projects in OpenIDEA:

- **Index** of every destination literal:
  - consumers: `@KafkaListener(topics = …)`, `@RabbitListener(queues = …)` (plus `queuesToDeclare = @Queue(…)` and `bindings = @QueueBinding(value = @Queue(…), exchange = @Exchange(…))`, which consume the queue and the exchange), `@JmsListener(destination = …)`;
  - producers: `send` of a Kafka template, `new ProducerRecord<>("topic", …)`, `convertAndSend`/`send`/`convertSendAndReceive` of a Rabbit template (`(routingKey, msg)` goes to the queue of that name through the default exchange, `(exchange, routingKey, msg)` to the exchange), `convertAndSend`/`send` of a JMS template, `@SendTo` on a listener method (`exchange/routingKey` for Rabbit);
  - declarations: `TopicBuilder.name`, `new NewTopic`, `new Queue`, `QueueBuilder.durable/nonDurable`, `new TopicExchange`/`DirectExchange`/`FanoutExchange`/`HeadersExchange`, `ExchangeBuilder.*Exchange`, `new ActiveMQQueue`/`ActiveMQTopic`.
- **Gutter arrows**: a consumer links to the producers and declarations of its destination, a producer to its consumers. Go to Definition on a destination literal lists every other place naming it.
- **Completion** of known destination names (same broker, same kind: topic, queue, exchange or JMS destination) inside those literals.
- **Messaging tool window**: broker → destination → consumers, producers and declarations; a click opens the place, Refresh rescans.

Matching is text-based, by exact name. The broker of a `send`/`convertAndSend` comes from the receiver's name (`kafkaTemplate`, `rabbitTemplate`, `jmsTemplate`…) or, failing that, the declared type of the field or parameter it names; calls on anything else (`streamBridge`, a messaging template) are left out. SpEL is skipped. Destinations written as a `static final String` (Java) or `const val` (Kotlin) constant, `Queues.ORDERS`, are read through the string constants of the project (literals and `+` chains in one file; a bare `NAME` only inside its own class); a constant the project does not define is dropped. There are no warnings for destinations, because a destination may be produced or consumed outside the project.

Placeholders: `${key}` and `${key:default}` in `topics`, `queues`, `destination`, `@SendTo` and in send calls are resolved from `application*`/`bootstrap*` `.properties`, `.yml` and `.yaml` files of the project (keys match relaxed: `app.topic-name` = `app.topicName`), a default or a value holding another placeholder down to two levels. `application.*` wins over profile files (`application-dev.*`), then the rest in path order; the active profile is unknown to plugins, so a value that differs per profile takes the first file's. A placeholder that can't be resolved, or that resolves to a comma-separated list, gets no link. Rabbit `@SendTo("${…}")` is skipped (the `exchange/routingKey` form can't be told apart).

XML listeners: `<jms:listener destination>` and `<rabbit:listener queue-names>` (bean-name `queues` are not read) in Spring XML files link their destination like annotated consumers; `ref` and `method` go to the bean (XML `<bean>`, stereotypes, `@Bean` methods) and its class's methods, and complete. Spring Kafka has no XML listener.

XML checks (hand-written tables, not read from the XSDs; only these elements): Rabbit `listener` needs `ref`, `listener-container` `acknowledge` is `auto|manual|none`, the four exchange elements need `name`, `queue`/exchange booleans (`durable`, `exclusive`, `auto-delete`) are `true|false`; JMS `listener` needs `destination` and `ref`, `listener-container` `acknowledge`, `container-type`, `destination-type` and `cache` take their schema values. A `ref` naming no bean of the project is Info only (library and container beans are invisible); a `method` the project class does not have is a warning, quiet when the class extends or implements anything. Values with `${}`/`#{}` are skipped.

Kotlin sources are indexed too (array literals, `suspend` and expression-bodied functions), except the nested `queuesToDeclare`/`bindings` annotations, which are read in Java only.

Not ported: broker connection and live topics, Spring Cloud Stream bindings (a separate plugin), SpEL, `BindingBuilder` routing, constants held in non-constant expressions or other projects' classes.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
