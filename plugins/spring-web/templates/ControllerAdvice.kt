package $PACKAGE$

import org.slf4j.LoggerFactory
import org.springframework.http.ResponseEntity
import org.springframework.web.bind.annotation.ControllerAdvice
import org.springframework.web.bind.annotation.ExceptionHandler

@ControllerAdvice
class $NAME$ {

    private val log = LoggerFactory.getLogger($NAME$::class.java)

    @ExceptionHandler(Exception::class)
    fun handle(exception: Exception): ResponseEntity<String> {
        log.error("Unhandled exception", exception)
        $END$
        return ResponseEntity.internalServerError().body("Internal server error")
    }
}
