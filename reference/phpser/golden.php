<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-phpser/tests/golden/php-serialize.tsv: values
// serialized by real PHP, the oracle for the Rust codec.
//
// Usage: php reference/phpser/golden.php > crates/hyscribe-phpser/tests/golden/php-serialize.tsv

ini_set('serialize_precision', '-1');

class Visibility {
    public $pub = 1;
    protected $prot = 'two';
    private $priv = [3];
}
class Legacy implements Serializable {
    public function serialize() { return 'payload:{}"'; }
    public function unserialize($data) {}
    public function __serialize(): array { return ['p' => 1]; }
    public function __unserialize(array $data): void {}
}
class OldStyle implements Serializable {
    public function serialize() { return 'raw;data'; }
    public function unserialize($data) {}
}
enum Suit { case Hearts; case Spades; }

$values = [
    'null' => null,
    'true' => true,
    'false' => false,
    'zero' => 0,
    'int max' => PHP_INT_MAX,
    'int min' => PHP_INT_MIN,
    'empty string' => '',
    'utf8 string' => "Hello, w\u{f6}rld \u{1F600}",
    'binary string' => "\0\x01\xff\"';{}",
    'empty array' => [],
    'list' => [1, 'two', 3.5, null],
    'map' => ['a' => 1, 10 => 'ten', '-5' => 'numeric string key', '05' => 'kept string key'],
    'nested' => ['x' => ['y' => ['z' => [true]]]],
    'stdClass' => (object) ['a' => 1, 'b' => [2]],
    'visibility' => new Visibility(),
    'magic serialize' => new Legacy(),
    'enum' => Suit::Spades,
    'enum list' => [Suit::Hearts, Suit::Hearts],
];
if (PHP_VERSION_ID < 90000) {
    $values['custom'] = @new OldStyle();
}
$shared = new stdClass();
$values['object handle'] = [$shared, $shared];
$a = 1;
$values['php reference'] = [&$a, &$a];
// WordPress shapes from a default install.
$values['active_plugins'] = ['akismet/akismet.php', 'hello.php'];
$values['user_roles'] = ['administrator' => ['name' => 'Administrator', 'capabilities' => ['switch_themes' => true, 'level_10' => true]]];

foreach ($values as $name => $value) {
    echo "value\t", $name, "\t", bin2hex(@serialize($value)), "\n";
}

mt_srand(20261027);
$floats = [0.0, -0.0, 0.1, 0.2, 0.3, 1/3, 2/3, 1e15, 1e16, 1e17, 1e-4, 1e-5, 123.456, -7.25e-9,
    PHP_FLOAT_MAX, PHP_FLOAT_MIN, PHP_FLOAT_EPSILON, 5e-324, INF, -INF];
for ($i = 0; $i < 2000; $i++) {
    $bits = pack('J', (mt_rand() << 32) ^ mt_rand() ^ (mt_rand() << 16));
    $f = unpack('E', $bits)[1];
    if (!is_nan($f)) {
        $floats[] = $f;
    }
    $floats[] = mt_rand() / mt_getrandmax() * (10 ** mt_rand(-12, 22));
}
foreach ($floats as $f) {
    echo "float\t", bin2hex(pack('E', $f)), "\t", bin2hex(serialize($f)), "\n";
}
