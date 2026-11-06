package $PACKAGE$

import org.springframework.stereotype.Controller
import org.springframework.ui.Model
import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.RequestMapping

@Controller
@RequestMapping("/")
class $NAME$ {

    @GetMapping
    fun index(model: Model): String {
        $END$
        return "index"
    }
}
