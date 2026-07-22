# App Fair — Privacy Policy

_Last updated: 2026-07-21_

App Fair is an open app store for F-Droid–compatible catalogs. It is designed to collect nothing
about you.

## What App Fair collects

**Nothing.** App Fair has no accounts, no sign-in, no analytics, no advertising, and no crash or
usage reporting. We do not collect, transmit, or sell any personal or device data.

## What leaves your device

App Fair connects only to the catalog repositories you choose to add (F-Droid's official
repository is included by default). When you open the app, browse, or install, App Fair requests:

- the catalog **index** (the list of apps and their metadata) from each enabled catalog;
- **icons and screenshots** referenced by that catalog;
- the **app package (APK)** you choose to install or update, from that catalog.

These requests go directly from your device to the catalog's servers over HTTPS. The catalog
operator may see standard request information (such as your IP address and the file requested),
exactly as your web browser would reveal when visiting their site. App Fair adds no identifiers to
these requests and sends nothing about what you browse or install anywhere else.

## What stays on your device

The synced catalog, your list of catalogs, your preferences, and a record of what you installed
through App Fair are stored **only** on your device. Removing the app removes this data.

## Installing apps

App Fair downloads an app's package from its catalog, verifies it against the catalog's published
checksum, and hands it to Android's standard package installer, which asks you to confirm every
installation. App Fair never installs anything silently.

## Permissions

- **Internet** — to fetch catalog indexes, images, and app packages from the catalogs you add.
- **Install packages** (`REQUEST_INSTALL_PACKAGES`) — to hand a downloaded package to Android's
  installer for your confirmation.
- **Query installed packages** (`QUERY_ALL_PACKAGES`) — to tell whether a catalog app is already
  installed and whether an update is available, so it can show "Install" versus "Update".

## Children

App Fair is not directed to children and does not knowingly collect any data from anyone.

## Contact

Questions about this policy: <privacy@appfair.org>.

---

> **Note for release:** Google Play requires a privacy policy reachable at a public **URL** (entered
> in Play Console → App content → Privacy policy). Publish this text at a stable URL (for example
> `https://appfair.org/privacy`) and enter that URL in the console. Update the contact address above
> to a real, monitored inbox before submitting.
