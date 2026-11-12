// Downloads the OCR language models into src-tauri/tessdata/ (bundled with the app).
// Usage: node scripts/fetch-tessdata.mjs [langs…]   (default: fra eng deu ita)
// Models: https://github.com/tesseract-ocr/tessdata_best (Apache 2.0 licence).
import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

/** fetch with retries: CI runners sometimes time out connecting to GitHub. */
async function fetchRetry(url, tries = 5) {
  for (let i = 1; ; i++) {
    try {
      const res = await fetch(url)
      if (res.ok || res.status < 500 || i >= tries) return res
    } catch (e) {
      if (i >= tries) throw e
    }
    console.log(`Nouvel essai (${i}/${tries - 1})…`)
    await new Promise((r) => setTimeout(r, 3000 * i))
  }
}

const langs = process.argv.length > 2 ? process.argv.slice(2) : ['fra', 'eng', 'deu', 'ita']
const out = join(import.meta.dirname, '..', 'src-tauri', 'tessdata')
mkdirSync(out, { recursive: true })

for (const lang of [...langs, 'osd']) {
  const file = join(out, `${lang}.traineddata`)
  if (existsSync(file)) {
    console.log(`${lang} : déjà présent`)
    continue
  }
  // osd (page orientation) only exists in the regular repository.
  const repo = lang === 'osd' ? 'tessdata' : 'tessdata_best'
  const url = `https://raw.githubusercontent.com/tesseract-ocr/${repo}/main/${lang}.traineddata`
  const res = await fetchRetry(url)
  if (!res.ok) throw new Error(`Téléchargement impossible (${res.status}) : ${url}`)
  writeFileSync(file, Buffer.from(await res.arrayBuffer()))
  console.log(`${lang} : OK`)
}
writeFileSync(
  join(out, 'LICENSE-tessdata.txt'),
  'Tesseract language models, https://github.com/tesseract-ocr/tessdata_best\nLicensed under the Apache License, Version 2.0.\n',
)
