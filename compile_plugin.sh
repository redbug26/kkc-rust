#!/bin/bash

SRC="assets/plugins"
DEST="stuff"
ROOT="$(cd "$(dirname "$0")" && pwd)"

# créer le dossier de destination s'il n'existe pas
mkdir -p "$DEST"

# parcourir les sous-dossiers
for dir in "$SRC"/*/; do
    # enlever le slash final et récupérer le nom du dossier
    dirname=$(basename "$dir")

    echo "Processing $dirname..."

    # créer le zip (.kkplug)
    

    (
        cd "$dir" || exit
        zip -r "$ROOT/$DEST/$dirname.kkplug" .
    )

done

echo "Done."
