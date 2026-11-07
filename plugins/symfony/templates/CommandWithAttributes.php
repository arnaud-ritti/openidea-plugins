<?php

namespace App\Command;

use Symfony\Component\Console\Attribute\Argument;
use Symfony\Component\Console\Attribute\AsCommand;
use Symfony\Component\Console\Attribute\Option;
use Symfony\Component\Console\Command\Command;
use Symfony\Component\Console\Style\SymfonyStyle;

#[AsCommand(name: 'app:$END$', description: 'Describe what this command does')]
class $NAME$
{
    public function __invoke(
        SymfonyStyle $io,
        #[Argument(description: 'Argument description')] ?string $arg1 = null,
        #[Option(description: 'Option description')] bool $option1 = false,
    ): int {
        if ($arg1) {
            $io->note(sprintf('You passed an argument: %s', $arg1));
        }

        if ($option1) {
            $io->note('You passed the option.');
        }

        $io->success('Done.');

        return Command::SUCCESS;
    }
}
