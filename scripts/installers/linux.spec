Name: atelier-kanban
Version: %{release_version}
Release: 1
Summary: Tableau de tâches local Atelier Kanban
License: AGPL-3.0-or-later AND MIT
URL: https://github.com/%{repository}
Requires: libsecret, xdg-utils, lsof, zenity
# Preserve the bundled, tested Node binary and generate its shared-library requirements.
%global __os_install_post %{nil}

%description
Tableau Kanban local avec Node.js inclus. Les données restent dans le dossier personnel.

%install
mkdir -p "%{buildroot}"
cp -a "%{payload}/." "%{buildroot}/"

%pre
%{check_stopped}

%preun
%{check_stopped}

%files
%defattr(-,root,root,-)
/opt/atelier-kanban
/usr/bin/atelier-kanban
/usr/share/applications/atelier-kanban.desktop
/usr/share/icons/hicolor/scalable/apps/atelier-kanban.svg
