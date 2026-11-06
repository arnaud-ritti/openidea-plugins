package $PACKAGE$

import org.springframework.http.ResponseEntity
import org.springframework.web.bind.annotation.ControllerAdvice
import org.springframework.web.bind.annotation.ExceptionHandler

@ControllerAdvice
class $NAME$ {

    @ExceptionHandler(Exception::class)
    fun handle(exception: Exception): ResponseEntity<String> {
        $END$
        return ResponseEntity.internalServerError().body(exception.message)
    }
}
