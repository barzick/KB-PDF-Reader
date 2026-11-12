// Downloads the PDFium library for this machine (or the one given) into src-tauri/pdfium/.
// Usage: node scripts/fetch-pdfium.mjs [mac-arm64|mac-x64|win-x64|win-arm64|linux-x64]
// PDFium binaries: https://github.com/bblanchon/pdfium-binaries (BSD/Apache licences).
import { execSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const VERSION = 'chromium/8152'

const auto = () => {
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
  if (process.platform === 'darwin') return `mac-${arch}`
  if (process.platform === 'win32') return `win-${arch}`
  return `linux-${arch}`
}
const target = process.argv[2] ?? auto()
const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/${encodeURIComponent(VERSION)}/pdfium-${target}.tgz`
const out = join(import.meta.dirname, '..', 'src-tauri', 'pdfium')
const tmp = mkdtempSync(join(tmpdir(), 'pdfium-'))

console.log(`PDFium ${VERSION} (${target}) → ${out}`)
const res = await fetch(url)
if (!res.ok) throw new Error(`Téléchargement impossible (${res.status}) : ${url}`)
writeFileSync(join(tmp, 'pdfium.tgz'), Buffer.from(await res.arrayBuffer()))
execSync(`tar -xzf pdfium.tgz`, { cwd: tmp, stdio: 'inherit' })

rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })
const lib = target.startsWith('win')
  ? join(tmp, 'bin', 'pdfium.dll')
  : join(tmp, 'lib', target.startsWith('mac') ? 'libpdfium.dylib' : 'libpdfium.so')
if (!existsSync(lib)) throw new Error(`Bibliothèque introuvable dans l'archive : ${lib}`)
cpSync(lib, join(out, lib.split(/[\\/]/).pop()))
cpSync(join(tmp, 'LICENSE'), join(out, 'LICENSE-pdfium.txt'))
if (existsSync(join(tmp, 'licenses'))) cpSync(join(tmp, 'licenses'), join(out, 'licenses'), { recursive: true })
rmSync(tmp, { recursive: true, force: true })
console.log('OK')
