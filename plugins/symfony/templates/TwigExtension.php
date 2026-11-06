<?php

namespace App\Twig;

use Twig\Extension\AbstractExtension;
use Twig\TwigFilter;
use Twig\TwigFunction;

class $NAME$ extends AbstractExtension
{
    public function getFilters(): array
    {
        return [
            new TwigFilter('$END$', [$this, 'doSomething']),
        ];
    }

    public function getFunctions(): array
    {
        return [
            new TwigFunction('function_name', [$this, 'doSomething']),
        ];
    }

    public function doSomething(string $value): string
    {
        return $value;
    }
}
