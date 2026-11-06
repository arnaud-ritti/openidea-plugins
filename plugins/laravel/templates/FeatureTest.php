<?php

test('$NAME$', function () {
    $response = $this->get('/');

    $response->assertStatus(200);
    $END$
});
