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
- Liste des polices utilisées (intégrées ou non) : base de la future recherche automatique de polices
- Enregistrer / Enregistrer sous

## Feuille de route

1. Recherche automatique des polices : PDF → ordinateur → téléchargement (Google Fonts, Fontsource) → équivalent métrique (Liberation, Carlito…)
2. Édition du texte existant (paragraphes, retour à la ligne, police d'origine), des images et des formes
3. Annotations, surlignage, formulaires, signatures
4. OCR des documents scannés (Tesseract), avec texte modifiable
5. Création de PDF (pages vierges, images, documents)

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
