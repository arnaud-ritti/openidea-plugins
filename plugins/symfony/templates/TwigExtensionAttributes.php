<?php

namespace App\Twig;

use Twig\Attribute\AsTwigFilter;
use Twig\Attribute\AsTwigFunction;

class $NAME$
{
    #[AsTwigFilter('$END$')]
    public function doSomething(string $value): string
    {
        return $value;
    }

    #[AsTwigFunction('function_name')]
    public function functionName(string $value): string
    {
        return $value;
    }
}
