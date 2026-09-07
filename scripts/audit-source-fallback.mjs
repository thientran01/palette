// Read-only local-cache audit using the production parser and fallback.
// Usage: node scripts/audit-source-fallback.mjs <app-data-directory> [--details]
// Audits each source-matching cache variant independently (not unique songs).
// Missing rows already display as source lines; count them separately from changes.
// Track duration is unavailable: duration-dependent outro boundaries are not
// synthesized. Final-line fallback counts can therefore be lower than in the app.
// Outputs timing metadata only, never lyric text or audio.
import fs from "node:fs";
import path from "node:path";
import Module from "node:module";
import ts from "typescript";
const root = process.argv[2];
if (!root) throw new Error("Pass the Palette app-data directory");
const file = path.resolve("src/lib/lrc.ts");
const compiled = ts.transpileModule(fs.readFileSync(file, "utf8"), {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2020}}).outputText;
const mod = new Module(file); mod._compile(compiled,file);
const {parseLrc,attachWords} = mod.exports;
const report = {limitation:"Track duration unavailable; duration-dependent outro boundaries are omitted, so final-line fallback counts may be understated.", cacheRecords:0, lines:0, suppressedWordLines:0, reasons:{}, byVariant:{}, examples:[]};
for (const dir of ["karaoke-vocals-preview-v2","karaoke-vocals-preview-v1","karaoke"]) {
  const folder=path.join(root,dir);
  if (!fs.existsSync(folder)) continue;
  for (const name of fs.readdirSync(folder).filter(n=>n.endsWith(".json"))) {
    let cache,source;
    try {cache=JSON.parse(fs.readFileSync(path.join(folder,name),"utf8"));source=JSON.parse(fs.readFileSync(path.join(root,"lyrics",name),"utf8")).synced;} catch {continue;}
    if (!source || cache.synced!==source || !Array.isArray(cache.words) || !cache.words.length) continue;
    const lines=parseLrc(source,0), result=attachWords(lines,cache.words);
    report.cacheRecords++;
    const variant=report.byVariant[dir] ??= {cacheRecords:0,lines:0,suppressedWordLines:0,missing:0};
    variant.cacheRecords++;
    result.forEach((line,i)=>{
      if (!line.text || line.end!==undefined) return;
      report.lines++; variant.lines++;
      if (!line.alignmentFallback) return;
      const reason=line.alignmentFallback;
      if (reason === "missing") variant.missing++;
      else { report.suppressedWordLines++; variant.suppressedWordLines++; }
      report.reasons[reason]=(report.reasons[reason]??0)+1;
      if(process.argv.includes("--details")) report.examples.push({cache:dir,key:name.slice(0,-5),line:line.t,next:lines[i+1]?.t,reason});
    });
  }
}
console.log(JSON.stringify(report,null,2));
