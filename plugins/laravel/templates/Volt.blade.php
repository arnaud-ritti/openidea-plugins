<?php

use function Livewire\Volt\{state};

state(['count' => 0]);

$increment = fn () => $this->count++;

?>

<div>
    <button wire:click="increment">{{ $count }}</button>
    $END$
</div>
