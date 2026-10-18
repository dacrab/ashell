%define __spec_install_post %{nil}
%define __os_install_post %{_dbpath}/brp-compress
%define debug_package %{nil}

Name:           ashell
Summary:        A ready to go Wayland status bar for Hyprland
Version:        @@VERSION@@
Release:        @@RELEASE@@%{?dist}
License:        GPL-3.0-or-later
URL:            https://github.com/MalpenZibo/ashell
Source0:        %{name}-%{version}.tar.gz

%description
%{summary}

%prep
%setup -q

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a * %{buildroot}

%files
%license %{_datadir}/licenses/ashell/LICENSE
%{_bindir}/*

%changelog
* Sun Oct 18 2026 dacrab <vaggeliskavouras6@gmail.com> - @@VERSION@@-@@RELEASE@@
- Automated build from main
