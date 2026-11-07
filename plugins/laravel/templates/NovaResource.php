<?php

namespace App\Nova;

use Illuminate\Http\Request;
use Laravel\Nova\Fields\ID;
use Laravel\Nova\Http\Requests\NovaRequest;

class $NAME$ extends Resource
{
    public static $model = \App\Models\$NAME$::class;

    public static $title = 'id';

    public static $search = ['id'];

    public function fields(NovaRequest $request): array
    {
        return [
            ID::make()->sortable(),
            $END$
        ];
    }
}
