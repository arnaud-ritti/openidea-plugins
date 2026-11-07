<?php

namespace App\Command;

use Symfony\Component\Console\Attribute\AsCommand;
use Symfony\Component\Console\Command\Command;
use Symfony\Component\Console\Style\SymfonyStyle;

#[AsCommand(name: 'app:$END$', description: 'Describe what this command does')]
class $NAME$
{
    public function __invoke(SymfonyStyle $io): int
    {
        $io->success('Done.');

        return Command::SUCCESS;
    }
}
