<?php

namespace App\Tests;

use Symfony\Bundle\FrameworkBundle\Test\KernelTestCase;

class $NAME$ extends KernelTestCase
{
    public function testSomething(): void
    {
        self::bootKernel();

        $container = static::getContainer();

        $this->assertSame('test', $container->getParameter('kernel.environment'));
        $END$
    }
}
