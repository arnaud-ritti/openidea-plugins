<?php

namespace App\Exceptions;

use Symfony\Component\HttpKernel\Exception\HttpException;

class $NAME$ extends HttpException
{
    public function __construct(string $message = '', ?\Throwable $previous = null)
    {
        parent::__construct(400, $message, $previous);
    }
}
