# Spring Batch Tools

Spring Batch job configuration in OpenIDEA:

- **XML jobs** (the `http://www.springframework.org/schema/batch` namespace, any prefix or the default one): Go to Definition and completion on `next`, `to` and `restart` (the step, flow, split or decision of the same job), and on `parent` (the same job, then the file, then the project). A transition to an id the job doesn't have is an error.
- **Bean references**: `ref` (tasklet, listener, job step), `reader`, `writer`, `processor`, `skip-policy`, `retry-policy`, `decider`, `job-repository`, `transaction-manager`, `task-executor`, `job-launcher`, `partitioner`, `chunk-completion-policy` open the bean (`<bean id>` or a Java/Kotlin bean name), complete bean names and warn when no bean has that name. Common infrastructure beans (`jobRepository`, `transactionManager`, `dataSource`…) and `${…}`/`#{…}` values are never reported, and nothing is reported until the first full scan of the project.
- **`spring.batch.job.name`** in `application*.properties`/`.yml`: Go to Definition to the job (`<job id>`, `new JobBuilder("name", …)` or `jobBuilderFactory.get("name")`), completion of job names, and a warning for each unknown name in the comma-separated list.
- **Spring Batch 5 hints** in Java: `JobBuilderFactory`/`StepBuilderFactory` and their `get(…)` calls are struck through as deprecated (use `new JobBuilder(name, jobRepository)` / `new StepBuilder(name, jobRepository)`); `@EnableBatchProcessing` in a project with a `@SpringBootApplication` class gets a note that it switches off Boot's batch auto-configuration.

Matching is text-based: names as written in attributes and string literals. Jobs and beans from libraries, auto-configuration or other modules aren't seen.

Not ported: job and flow diagrams, Spring Batch Admin, job parameters and late-binding SpEL (`#{jobParameters[...]}`) checks, Kotlin configuration (Kotlin sources only count as bean declarations).

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
