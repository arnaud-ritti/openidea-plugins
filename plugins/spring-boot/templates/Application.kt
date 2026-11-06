package $PACKAGE$

import org.springframework.boot.autoconfigure.SpringBootApplication
import org.springframework.boot.runApplication

@SpringBootApplication
class $NAME$

fun main(args: Array<String>) {
    runApplication<$NAME$>(*args)
}
