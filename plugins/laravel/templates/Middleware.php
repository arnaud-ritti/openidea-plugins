<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;

class $NAME$
{
    public function handle(Request $request, Closure $next): Response
    {
        $END$

        return $next($request);
    }
}
