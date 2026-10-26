# Mises à jour des CLI — candidat R03

Le diagnostic explicite d’un profil Windows lit sa version et les métadonnées publiques du paquet correspondant au registre npm HTTPS. Aucun runtime Node, compte npm, proxy implicite, redirection ou secret n’est utilisé pour cette lecture. Seules les versions stables à trois composantes sont acceptées. Un programme personnalisé ou une installation hors des dossiers natifs du fournisseur renvoie vers son gestionnaire habituel.

La confirmation porte sur le profil enregistré, le dossier natif, la version installée, la version publiée, la date du diagnostic et le SHA256 du programme. L’aperçu expire après dix minutes. Ces éléments sont relus avant l’envoi ; un changement refuse la commande. Les autres sessions de ce CLI doivent être fermées par l’utilisateur.

La commande native est uniquement `update`, sans prompt ni jeton dans son environnement filtré. Le programme reste épinglé jusqu’à sa création suspendue dans le Job Object ; son identité est vérifiée avant reprise. Un verrou de fichier à lien unique protège le dossier contre les mises à jour concurrentes d’autres instances Atelier. La capture est bornée à 180 secondes et 1 Mio par flux ; aucune sortie brute n’est affichée.

Le journal SQLite distingue préparation, commande en cours, version observée, refus avant lancement et résultat incertain. Après interruption, aucune commande n’est rejouée : un nouveau diagnostic peut constater la version actuelle sans prouver la réussite de la commande passée. Une version relue inférieure à la cible reste explicitement signalée. Le fournisseur peut installer une version stable plus récente que l’aperçu ; aucun rollback de CLI n’est promis.

Runs, services, opérations distantes, file active, minuteurs, exports, sauvegardes/restaurations et mises à jour d’Atelier sont protégés contre le chevauchement. Les changements de profil ou de diagnostic retirent la confirmation dans l’interface bilingue.

État : implémentation candidate Windows et scénarios purs/SQLite/HTTP locaux/fichier épinglé écrits. **Aucun build, test, scénario navigateur ou workflow exécuté.** Parcours positif de mise à jour d’un faux CLI, refus avant reprise du processus, interruption et interface réelle restent à qualifier dans la campagne finale. POSIX n’est pas pris en charge par cet incrément.

Référence : commande officielle Codex [`update`](https://learn.chatgpt.com/docs/cli/reference), et commandes natives de la référence fonctionnelle `lib/agent-updates.mjs`. Aucune installation initiale de CLI ni capacité Azure supplémentaire n’est ajoutée.
