# Parcours de projet Rust — candidat R05

Le projet enregistre une commande de vérification, une branche cible, l’autorisation des parcours et l’option de correction P0/P1 (deux cycles maximum). Leur modification utilise les mêmes préconditions de concurrence que les autres réglages ; les options booléennes ont des préconditions typées. L’enregistrement ne lance rien.

Depuis une tâche avec worktree prêt, une confirmation lie le besoin, le profil public et le projet affichés. Le parcours persiste son identifiant, ses étapes et les références aux runs. Le superviseur commun enchaîne développement puis vérification. Chaque lancement est réservé avant sa préparation ; un arrêt accepté entre réservation et démarrage rend ce lancement inutilisable. Les agents gardent leurs interactions, journaux bornés et arrêt des descendants existants.

Après vérification réussie, le parcours attend le contrôle explicite du commit, du push et de la PR/MR dans les panneaux existants. Il ne rejoue aucune publication. Après confirmation d’une PR/MR liée aux branches attendues, il revérifie les sources puis lance la review. Son résultat est revalidé contre les commits source/cible/base et le besoin actuels ; les sources doivent encore correspondre à la vérification. Les constats P0/P1 peuvent déclencher deux corrections au maximum, chacune suivie de vérifications et d’un nouveau contrôle de publication. Les autres résultats restent prêts pour contrôle humain ou bloqués lorsque les critères ne sont pas couverts. Aucun critère humain n’est coché automatiquement.

Un redémarrage interrompt les parcours actifs sans nouveau lancement. La reprise est explicite et refuse un besoin, un profil, un projet ou un worktree changé. Une étape de publication/review interrompue reprend par les vérifications. Les opérations incompatibles sur la tâche, les projets, les profils, les sauvegardes ou les mises à jour sont bloquées pendant l’activité. L’arrêt du parcours reste disponible via une route d’annulation exacte. Les données restaurées restent interrompues et leurs reviews/vérifications périmées.

## État des preuves

- Implémentation candidate Windows : stockage, superviseur partagé avec la file, panneau de tâche et réglages bilingues.
- Scénarios de stockage écrits : réservation unique, annulation avant lancement tardif, redémarrage sans rejeu, reprise refusée après modification, attente avant publication, vérification périmée, corrections bornées et critères humains conservés. Leurs résultats synthétiques injectés représentent des états de fixture, aucune exécution réelle d’agent ou de commande.
- Scénario WebDriver écrit : faux agent compilé, développement puis vraie commande synthétique prévue, attente de publication, refus sans PR/MR, arrêt et rechargement. Il n’a pas été exécuté.
- Aucun build, test, Clippy, navigateur ou workflow lancé pendant cet incrément. Formatage et inventaire uniquement ; aucune preuve nouvelle de réussite.

Restent ouverts : publication entièrement orchestrée avec auteur/fichiers/contenu figés et reprise des effets incertains, parcours heureux complet avec doubles de forge, intégration des parcours à la file par projet, qualification POSIX et campagne finale. Ce candidat ne termine pas R05.
