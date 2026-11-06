package $PACKAGE$;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/")
public class $NAME$ {

    @GetMapping
    public String index() {
        $END$
        return "";
    }
}
