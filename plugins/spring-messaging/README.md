# Spring Messaging Tools

Producer ↔ consumer navigation for Spring Kafka, Spring AMQP (RabbitMQ) and Spring JMS (ActiveMQ/Artemis) in Java projects in OpenIDEA:

- **Index** of every destination literal:
  - consumers: `@KafkaListener(topics = …)`, `@RabbitListener(queues = …)` (plus `queuesToDeclare = @Queue(…)` and `bindings = @QueueBinding(value = @Queue(…), exchange = @Exchange(…))`, which consume the queue and the exchange), `@JmsListener(destination = …)`;
  - producers: `send` of a Kafka template, `new ProducerRecord<>("topic", …)`, `convertAndSend`/`send`/`convertSendAndReceive` of a Rabbit template (`(routingKey, msg)` goes to the queue of that name through the default exchange, `(exchange, routingKey, msg)` to the exchange), `convertAndSend`/`send` of a JMS template, `@SendTo` on a listener method (`exchange/routingKey` for Rabbit);
  - declarations: `TopicBuilder.name`, `new NewTopic`, `new Queue`, `QueueBuilder.durable/nonDurable`, `new TopicExchange`/`DirectExchange`/`FanoutExchange`/`HeadersExchange`, `ExchangeBuilder.*Exchange`, `new ActiveMQQueue`/`ActiveMQTopic`.
- **Gutter arrows**: a consumer links to the producers and declarations of its destination, a producer to its consumers. Go to Definition on a destination literal lists every other place naming it.
- **Completion** of known destination names (same broker, same kind: topic, queue, exchange or JMS destination) inside those literals.
- **Messaging tool window**: broker → destination → consumers, producers and declarations; a click opens the place, Refresh rescans.

Matching is text-based, by exact name. The broker of a `send`/`convertAndSend` comes from the receiver's name (`kafkaTemplate`, `rabbitTemplate`, `jmsTemplate`…) or, failing that, the declared type of the field or parameter it names; calls on anything else (`streamBridge`, a messaging template) are left out. `${…}` placeholders, SpEL and names held in constants are skipped. There are no warnings, because a destination may be produced or consumed outside the project.

Not ported: broker connection and live topics, Spring Cloud Stream bindings (a separate plugin), Kotlin, XML listener containers, SpEL and placeholder resolution, `BindingBuilder` routing.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
