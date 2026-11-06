<?php

namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\Routing\Annotation\Route;

class $NAME$ extends AbstractController
{
    /**
     * @Route("/$END$", name="app_index")
     */
    public function index(): Response
    {
        return $this->render('index.html.twig');
    }
}
