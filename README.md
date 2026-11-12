# KB PDF Reader

Application de bureau pour **macOS et Windows** pour lire, assembler et modifier des PDF.

- **Interface** : Tauri 2 + React/TypeScript (`src/`)
- **Application de bureau** : Rust (`src-tauri/`)
- **Moteur PDF** : `crates/kb-core`, construit au-dessus de [PDFium](https://pdfium.googlesource.com/pdfium/), le moteur PDF de Chrome (licence BSD/Apache, intégré dans l'application : rien d'autre à installer pour l'utilisateur).

## Fonctions disponibles

- Ouvrir plusieurs PDF (bouton, glisser-déposer, « Ouvrir avec »), onglets
- Affichage fidèle par PDFium, miniatures, zoom, ajustement à la largeur
- Fusionner plusieurs PDF dans l'ordre choisi
- Pages : réorganiser (glisser les miniatures), pivoter, supprimer, extraire
- **Recherche automatique des polices** (onglet Polices) : pour chaque police du PDF, l'app cherche une version complète sur l'ordinateur, sinon la télécharge gratuitement (Fontsource / Google Fonts, gardée en cache), sinon utilise un équivalent aux mêmes largeurs (Arimo/Liberation pour Arial et Helvetica, Tinos pour Times, Cousine pour Courier, Carlito pour Calibri, Caladea pour Cambria…). Téléchargement désactivable.
- Enregistrer / Enregistrer sous

## Feuille de route

1. Édition du texte existant (paragraphes, retour à la ligne, police d'origine), des images et des formes
2. Annotations, surlignage, formulaires, signatures
3. OCR des documents scannés (Tesseract), avec texte modifiable
4. Création de PDF (pages vierges, images, documents)

## Développement

Prérequis : Node 22, Rust stable, et sur Linux les dépendances Tauri (`libwebkit2gtk-4.1-dev`).

```bash
npm install
npm run pdfium        # télécharge PDFium pour votre machine dans src-tauri/pdfium/
npm run tauri dev     # lance l'application
npm run test:core     # tests du moteur (PDFIUM_DIR=src-tauri/pdfium)
npm run tauri build   # crée l'installateur (.dmg ou .msi/.exe)
```

Les installateurs macOS (Apple Silicon et Intel) et Windows sont aussi construits automatiquement par GitHub Actions à chaque modification (onglet **Actions** → dernier « Build » → *Artifacts*).

## Licences tierces

PDFium : BSD-3-Clause / Apache-2.0, voir `src-tauri/pdfium/LICENSE-pdfium.txt` (copié dans l'application).
