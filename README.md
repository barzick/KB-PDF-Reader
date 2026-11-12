# KB PDF Reader

Application de bureau pour **macOS et Windows** pour lire, assembler et modifier des PDF.

- **Interface** : Tauri 2 + React/TypeScript (`src/`)
- **Application de bureau** : Rust (`src-tauri/`)
- **Moteur PDF** : `crates/kb-core`, construit au-dessus de [PDFium](https://pdfium.googlesource.com/pdfium/), le moteur PDF de Chrome (licence BSD/Apache, intégré dans l'application : rien d'autre à installer pour l'utilisateur).
- **OCR** : [Tesseract](https://github.com/tesseract-ocr/tesseract) 5 (Apache 2.0) et Leptonica (BSD), compilés dans l'application, avec les modèles français, anglais, allemand et italien.

## Fonctions disponibles

- Ouvrir plusieurs PDF (bouton, glisser-déposer, « Ouvrir avec »), onglets
- Affichage fidèle par PDFium, miniatures, zoom, ajustement à la largeur
- Fusionner plusieurs PDF dans l'ordre choisi
- Pages : réorganiser (glisser les miniatures), pivoter, supprimer, extraire
- **Recherche automatique des polices** (onglet Polices) : pour chaque police du PDF, l'app cherche une version complète sur l'ordinateur, sinon la télécharge gratuitement (Fontsource / Google Fonts, gardée en cache), sinon utilise un équivalent aux mêmes largeurs (Arimo/Liberation pour Arial et Helvetica, Tinos pour Times, Cousine pour Courier, Carlito pour Calibri, Caladea pour Cambria…). Téléchargement désactivable.
- **Modifier le texte existant** : bouton « Modifier le texte », clic sur un paragraphe, saisie, Ctrl/⌘+Entrée. Le paragraphe est réécrit dans sa police (trouvée par la recherche automatique), sa taille et sa couleur, avec retour à la ligne automatique. Le reste de la page reste strictement identique (modification chirurgicale du contenu PDF). Option « Décaler la suite » pour déplacer ce qui est en dessous.
  Les paragraphes qui mélangent gras, italique, tailles ou couleurs gardent leurs styles, et le texte justifié reste justifié.
- **OCR des documents scannés** : l'app repère les pages scannées et propose « Reconnaître le texte ». Le texte devient cherchable, sélectionnable et copiable sans changer l'aspect de la page, et peut ensuite être **modifié** comme un texte normal (les mots scannés sont masqués à la couleur du papier et le nouveau texte est écrit à la couleur de l'encre).
- **Créer un PDF** (bouton « Nouveau ») : depuis des photos ou des scans (JPEG gardés sans perte de qualité, PNG, TIFF…, aussi par glisser-déposer d'images), ou document vierge. « + Page » insère une page vierge.
- Annuler (Ctrl/⌘+Z) sur toutes les modifications
- Enregistrer / Enregistrer sous

## Feuille de route

1. Ajouter du texte et des images sur une page, modifier images et formes, sous-ensemble des polices intégrées (fichiers plus légers)
2. Annotations, surlignage, formulaires, signatures
3. Recherche dans le document
4. Création de PDF depuis des documents Word / images en lot

## Développement

Prérequis : Node 22, Rust stable, et sur Linux les dépendances Tauri (`libwebkit2gtk-4.1-dev`).

```bash
npm install
npm run pdfium        # télécharge PDFium pour votre machine dans src-tauri/pdfium/
npm run tessdata      # télécharge les modèles OCR dans src-tauri/tessdata/
npm run tauri dev     # lance l'application
npm run test:core     # tests du moteur (PDFIUM_DIR=src-tauri/pdfium)
npm run tauri build   # crée l'installateur (.dmg ou .msi/.exe)
```

Tesseract est compilé à partir de ses sources au premier build (CMake et un compilateur C++ sont nécessaires, quelques minutes, puis mis en cache). Pour utiliser plutôt le Tesseract installé sur la machine (`apt install libtesseract-dev` ou `brew install tesseract`) : ajouter `--features system-tesseract` aux commandes cargo.

Les installateurs macOS (Apple Silicon et Intel) et Windows sont aussi construits automatiquement par GitHub Actions à chaque modification (onglet **Actions** → dernier « Build » → *Artifacts*).

## Licences tierces

PDFium : BSD-3-Clause / Apache-2.0, voir `src-tauri/pdfium/LICENSE-pdfium.txt` (copié dans l'application).
Tesseract et ses modèles de langue : Apache-2.0. Leptonica : BSD-2-Clause.
