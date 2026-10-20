<?php
// Anvil's Database viewer: Adminer (adminer.php beside this file) through its documented
// adminer_object() customization. Written by Anvil (src/dbviewer.rs); edits are replaced.

/** Whether Anvil vouches for this connection: one of its own MySQL ("server") or
 * PostgreSQL ("pgsql") services on 127.0.0.1, or an app's SQLite file it knows of, as
 * Anvil lists them in ../servers.json. Those log in without a password. */
function anvil_local(string $driver, string $server, string $db): bool {
	static $known;
	$known ??= json_decode((string) @file_get_contents(__DIR__ . '/../servers.json'), true) ?: [];
	return $driver === 'sqlite'
		? in_array($db, $known['sqlite'] ?? [], true)
		: in_array("$driver $server", $known['servers'] ?? [], true);
}

// Anvil's Open link (`?anvil=1&server=…&username=…&db=…`) logs in as the login form
// would, with no password, then Adminer redirects to the address without `anvil`. Only
// from the address bar or Anvil, not from another site's page.
$anvil_login = false;
if (isset($_GET['anvil']) && !$_POST) {
	$driver = current(array_filter(['server', 'pgsql', 'sqlite'], fn($d) => isset($_GET[$d])));
	$from = $_SERVER['HTTP_SEC_FETCH_SITE'] ?? 'none';
	$db = (string) ($_GET['db'] ?? '');
	if ($driver && in_array($from, ['none', 'same-origin'], true) && anvil_local($driver, (string) $_GET[$driver], $db)) {
		$_POST['auth'] = [
			'driver' => $driver,
			'server' => (string) $_GET[$driver],
			'username' => (string) ($_GET['username'] ?? ''),
			'password' => '',
			'db' => $db,
		];
		$anvil_login = true;
	}
	unset($_GET['anvil']);
	$_SERVER['REQUEST_URI'] = preg_replace('~([?&])anvil=[^&]*&?~', '$1', $_SERVER['REQUEST_URI']);
}

function adminer_object() {
	class AnvilAdminer extends Adminer\Adminer {
		function login($login, $password) {
			return anvil_local(Adminer\DRIVER, Adminer\SERVER, Adminer\DB) ?: parent::login($login, $password);
		}

		function verifyLoginToken(): bool {
			return !$GLOBALS['anvil_login'];
		}

		// Nothing from adminer.org: no version check, and the page may not reach it.
		function verifyVersion(): bool {
			return false;
		}

		function csp(array $csp): array {
			foreach ($csp as &$c) {
				$c['connect-src'] = "'self'";
				$c['frame-src'] = "'self'";
			}
			return $csp;
		}
	}
	return new AnvilAdminer;
}

include __DIR__ . '/adminer.php';
