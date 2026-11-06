package $PACKAGE$;

import org.springframework.stereotype.Controller;
import org.springframework.ui.Model;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;

@Controller
@RequestMapping("/")
public class $NAME$ {

    @GetMapping
    public String index(Model model) {
        $END$
        return "index";
    }
}
