package $PACKAGE$;

import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.ControllerAdvice;
import org.springframework.web.bind.annotation.ExceptionHandler;

@ControllerAdvice
public class $NAME$ {

    @ExceptionHandler(Exception.class)
    public ResponseEntity<String> handle(Exception exception) {
        $END$
        return ResponseEntity.internalServerError().body(exception.getMessage());
    }
}
