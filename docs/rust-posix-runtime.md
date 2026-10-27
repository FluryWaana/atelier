# Runtime POSIX — candidat R02/R03/R04/R05

Les parcours agents, commandes, vérification, services, extensions, review, file et automatisation utilisent désormais leurs cas d’usage communs sous Linux/macOS. Les mêmes réservations SQLite, protections de mutation, limites de flux, masquage, délais, confirmations et récupérations sans rejeu restent appliqués. Les routes fichiers/worktrees ne refusent plus Unix après le portage de leurs adaptateurs.

## Processus et limites

Chaque commande possède un groupe POSIX distinct. Linux utilise `waitid(WNOWAIT)` pour observer le leader sans le récolter. Darwin utilise `proc_pidinfo(PROC_PIDT_SHORTBSDINFO)` avec observation des zombies et vérifie PID, parent et groupe du leader toujours possédé ; cela remplace le `waitid(WNOHANG)` refusé dans les vrais essais macOS. Le leader reste conservé jusqu'au nettoyage pour éviter de signaler un identifiant de groupe réutilisé. L’arrêt envoie SIGKILL au groupe puis vérifie l’absence de membre non zombie avant de récolter le leader. La lecture numérique de `/bin/ps` est sans shell/environnement hérité, bornée à deux secondes, 4 Mio et 100 000 lignes ; une inspection invalide n’est pas un succès partiel. La supervision conserve sa garde d’activité si le nettoyage échoue. Le traitement interactif attend ce propriétaire, jamais le `Child` brut qui récolterait le leader trop tôt. Le correctif Darwin reste à requalifier.

**Ce mécanisme ne contient pas un descendant qui appelle `setsid` ou change de groupe.** Une commande démarrant un démon peut donc échapper à l’arrêt d’Atelier. L’avertissement est visible dans les panneaux Agents, Services et Connecteurs. Il faut utiliser les programmes au premier plan et vérifier les démons avant restauration/mise à jour. Ce n’est pas une sandbox ; aucun filtre seccomp, service privilégié ou processus étranger n’est ajouté/arrêté pour masquer cette limite. Un crash OS ou l’arrêt brutal d’Atelier peut laisser des processus ; la récupération n’adopte ni ne tue aveuglément un PID persistant.

Le scénario compilé `posix-runtime` crée volontairement un descendant qui change de session : il doit rester observable après l’arrêt du groupe et se fermer par le pipe synthétique possédé par le test. Il démontre la limite, sans laisser volontairement de démon ni utiliser un compte/agent réel. Le succès éventuel de ce scénario ne qualifierait donc pas un confinement des descendants détachés.

## Dossiers, exécutables et ports

Le répertoire des runs/services est fixé par `fchdir` sur un descripteur conservé, après validation et avant réservation/spawn. Un déplacement de chemin ne redirige pas silencieusement les écritures. Les commandes de services/vérification utilisent explicitement `/bin/sh -c` avec l’environnement sélectionné existant ; les agents et extensions gardent un lancement direct, sans shell implicite.

La découverte des agents accepte seulement un exécutable ordinaire avec permission d’exécution : chemin explicite, emplacements connus du HOME/Homebrew, puis entrées absolues du PATH (256 maximum). Aucun dossier courant n’est recherché. Version, connexion et quotas réutilisent les contrôles bornés et le filtrage existants. Les mises à jour natives des CLI restent Windows ; Unix indique le gestionnaire habituel, sans annoncer une commande qui n’est pas raccordée.

POSIX n’offre pas le partage de fichier Windows qui interdit la modification d’une image ouverte. Chaque appel d’extension copie donc les octets dans un dossier privé, vérifie le SHA256 approuvé, fixe les permissions à 0500 et exécute cette copie. Une modification de source avant la copie refuse l’appel ; la source humaine n’est jamais remplacée. Les identifiants restent sur stdin. Les bibliothèques adjacentes au programme ne sont pas copiées : le module doit être autonome ou utiliser les bibliothèques système. Un acteur ayant les mêmes droits locaux n’est pas isolé du compte utilisateur.

Les ports Linux sont lus dans `/proc` à partir des descripteurs de sockets des PID sélectionnés : 65 536 descripteurs, 1 Mio par table et 16 Mio cumulés. macOS utilise `/usr/sbin/lsof` avec PID exacts, champs numériques IPv4/IPv6, sans résolution DNS, une limite de deux secondes et 1 Mio. Les membres du groupe sont relus avant/après ; seuls les listeners d’un membre encore présent sont exposés. Ce relevé ponctuel ne prouve ni réponse HTTP ni disponibilité continue.

## État de preuve

Scénarios écrits : nettoyage de descendants ordinaires, annulation et activité persistante, limite `setsid`, diagnostics compilés, protocoles interactifs Codex/Claude, vérification de sources, comptes synthétiques, copies d’extensions et mutation humaine, parseurs de ports et vrais listeners synthétiques. Les scénarios services utilisent le shell de la plateforme ; leur point d’entrée enfant est explicitement ignoré hors lancement par le scénario.

**Aucun build, test, Clippy, scénario navigateur ou workflow exécuté pendant ce portage.** Toutes ces fonctions restent candidates. Les parcours bureaux/navigateurs, performances, nettoyage réel et limites de chaque architecture doivent être lus dans les résultats de la campagne finale avant toute qualification.

Références : [`waitid` Linux](https://man7.org/linux/man-pages/man2/waitid.2.html), [champs `lsof`](https://github.com/lsof-org/lsof/blob/master/docs/manpage.md), et [en-tête Apple XNU](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h), qui indique que NOTE_TRACK/NOTE_CHILD ne sont plus supportés depuis macOS 10.5. Aucun suivi kqueue inexistant n’est présenté comme une protection.
