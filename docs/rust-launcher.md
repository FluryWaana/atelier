# Lanceur Rust — candidat non qualifié

Le binaire `atelier` propose maintenant `launch` (commande par défaut), `stop`, `serve`, `--help` et `--version`. `serve-prototype` reste une entrée de référence explicite ; seul ce chemin peut charger les assets legacy. Le service natif accepte uniquement les noms d’assets Leptos/WASM autorisés, capturés par le serveur après ouverture vérifiée. Aucun moteur Node n’est lancé.

Pendant la reconstruction, utiliser exclusivement des dossiers synthétiques lors de la campagne finale, par exemple :

```text
atelier launch --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets SYNTHETIC_RUST_UI --port 4317 --no-browser
atelier stop --data-dir EMPTY_SYNTHETIC_DIRECTORY --port 4317
atelier serve --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets SYNTHETIC_RUST_UI --port 4317
```

Ces commandes sont documentées, **pas exécutées sur le PC du propriétaire**. `serve` garde le serveur au premier plan pour un diagnostic de démarrage ; `launch` démarre en arrière-plan, sans fichier de log illimité. L’absence d’assets et les options inconnues/répétées sont refusées. Les chemins des données doivent être représentables en Unicode, sans conversion silencieuse.

Le lanceur interroge le port numérique loopback avec le client HTTP existant, sans proxy/redirection, et vérifie sa preuve HMAC avant toute commande authentifiée. La réponse authentifiée doit annoncer application, protocole `atelier-local/1` et même dossier canonique. Un autre port/service/dossier n’est jamais arrêté ni adopté. Le démarrage attend au plus 20 secondes et arrête uniquement son enfant créé si celui-ci n’est pas prêt ; l’arrêt local utilise `/api/shutdown`, exempté de la garde de mutations comme les autres annulations, puis attend la disparition de l’instance. Les runs/services et écritures acceptées sont fermés par le serveur existant, pas contournés par un kill du lanceur.

Sur Windows le serveur n’ouvre pas de console et le navigateur est ouvert par l’API Shell. Sur POSIX, le serveur dispose d’un groupe de processus séparé ; l’ouverture utilise `/usr/bin/xdg-open` ou `/usr/bin/open` avec le fichier de redirection privé déjà prévu pour la session, sans jeton dans leurs arguments. Aucun jeton n’est affiché dans le terminal. Une erreur d’ouverture du navigateur laisse le serveur disponible et ne prétend pas qu’il a été arrêté.

Sans override, le répertoire du binaire fournit `ui`. Le mode portable utilise son sous-dossier `data` ; les modes installés utilisent LOCALAPPDATA/Home Windows, XDG_DATA_HOME/Home Linux, ou `Library/Application Support/AtelierKanban/data` sous macOS. Ces sélections sont du code candidat ; aucun dossier personnel n’a été ouvert pendant ce développement. Le format SQLite Rust et les refus de répertoires inconnus restent actifs ; rien n’est converti ni effacé implicitement. L’identité/coffre interne du prototype reste à finaliser avec la livraison ; aucun import Node ni rétrocompatibilité ne sont exigés.

Scénarios écrits : parsing sans I/O, démarrage du vrai binaire sur fixtures synthétiques sans navigateur ni identifiant, réutilisation du même PID, refus d’un autre dossier/port non vérifié sans création de données, arrêt/redémarrage et persistance d’une conversation sans agent avec rotation de session, refus des assets legacy avant initialisation. **Aucun compilé/exécuté, aucune ouverture de navigateur effectuée, aucun paquet réel produit.** Les règles de ressources, la campagne finale unique et le workflow manuel restent inchangés.

Restent à livrer/qualifier : tray produit, sélection native des dossiers, vrai installateur/désinstallateur sur les cinq cibles, archivage/distribution, mise à jour Rust→Rust signée et récupération/rollback, diagnostics de lancement plus guidés et liaisons avec les services de bureau. Les exécutables de faisabilité restent exclus des paquets.
