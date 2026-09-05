"""Regression pins for audio/song clock confusion, resume loss and calibration taps.

Run: python -m unittest discover -s scripts -p test_karaoke_tap.py -v
Executes the actual generated JavaScript with Node built-ins only.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import wave

GENERATOR = Path(os.environ.get('TAP_GENERATOR', Path(__file__).with_name('karaoke_tap.py')))
NODE = r"""
const fs=require('node:fs'), vm=require('node:vm'), assert=require('node:assert/strict');
const input=JSON.parse(fs.readFileSync(0,'utf8'));
const elements={}, listeners=[], timers=[], storage=new Map(), downloads=[];
function el(id){return elements[id] ||= {textContent:'',className:'',style:{},
  paused:true,currentTime:0,playbackRate:1,plays:0,
  pause(){this.paused=true},play(){this.paused=false;this.plays++;return Promise.resolve()},
  click(){if(this.onclick)return this.onclick()},remove(){}};}
global.document={getElementById:el,createElement:el,body:{appendChild(){}},
 addEventListener(type,fn,capture=false){listeners.push({type,fn,capture})},
 removeEventListener(type,fn){const k=listeners.findIndex(x=>x.type===type&&x.fn===fn);if(k>=0)listeners.splice(k,1)}};
function key(code,repeat=false){const e={code,key:code==='KeyP'?'p':code,repeat,stopped:false,
 preventDefault(){},stopImmediatePropagation(){this.stopped=true}};
 for(const h of [...listeners].sort((x,y)=>Number(y.capture)-Number(x.capture))){if(!e.stopped)h.fn(e)}}
global.localStorage={getItem:k=>storage.get(k)??null,setItem:(k,v)=>storage.set(k,v)};
global.setInterval=()=>0;global.setTimeout=(fn)=>{timers.push(fn)};global.confirm=()=>true;
global.URL={createObjectURL:b=>{downloads.push(b);return 'blob:test'}};
global.FileReader=class{readAsText(f){this.result=f;this.onload()}};
global.window={AudioContext:class{
 constructor(){this.currentTime=0;global.ctx=this;this.destination={}}
 createOscillator(){return {frequency:{},connect:g=>g,start(){},stop(){}}}
 createGain(){return {gain:{},connect(){}}}close(){this.closed=true;return Promise.resolve()}
}};
vm.runInThisContext(input.setup||'');
vm.runInThisContext(input.script);
vm.runInThisContext('(async()=>{'+input.check+'})()').catch(e=>{console.error(e);process.exitCode=1});
"""


class TapPageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dump = Path(self.tmp.name) / 'dump'
        self.dump.mkdir()
        self.out = Path(self.tmp.name) / 'evidence'
        self.meta = {'artist': 'Artist', 'title': 'Song', 'sample_rate': 16000,
                     'map': {'intercept_ms': 4000, 'slope_ms': 0.075}}
        self.write('meta.json', json.dumps(self.meta))
        self.write('lyrics.lrc', '[00:10.00]one two\n[00:20.00]three four\n')
        # A candidate onset crosses the next line: line identity must not change.
        self.write('labels.template.txt', '10\t10\tone\n21\t21\ttwo\n22\t22\tthree\n23\t23\tfour\n')
        self.write('words.json', '{"words":[]}')
        (self.dump / 'pcm.i16').write_bytes(b'\x00\x00' * 160)

    def write(self, name, value):
        (self.dump / name).write_text(value, encoding='utf-8')

    def generate(self, success=True):
        result = subprocess.run([sys.executable, str(GENERATOR), str(self.dump), str(self.out)],
                                text=True, capture_output=True)
        if not success:
            self.assertNotEqual(result.returncode, 0)
            return result.stderr
        self.assertEqual(result.returncode, 0, result.stderr)
        return (self.out / 'tap.html').read_text(encoding='utf-8')

    def js(self, check, setup='', page=None):
        page = self.generate() if page is None else page
        script = re.search(r'<script>(.*?)</script>', page, re.S).group(1)
        result = subprocess.run(['node', '-e', NODE],
                                input=json.dumps({'script': script, 'setup': setup, 'check': check}),
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_reaction_corrected_song_clock_at_multiple_rates(self):
        self.js("""
          for(const rate of [0.5,0.8,1]){
            stamps=[];i=0;done=false;a.paused=false;a.currentTime=10;a.playbackRate=rate;
            stamp();assert.ok(Math.abs(stamps[0]-(4+1.2*(10-0.12*rate)))<1e-9);
          }
          document.getElementById('dl').click();
          const text=await downloads[0].text();assert.ok(text.startsWith('# clock: song\\n'));
          assert.equal(Number(text.split('\\n')[1].split('\\t')[0]),15.856);
        """)

    def test_all_seek_controls_use_inverse_map(self):
        self.js("""
          const near=(x,y)=>assert.ok(Math.abs(x-y)<1e-9,`${x} != ${y}`);
          el('start').click();near(a.currentTime,4); // (10 - 1.2 - 4) / 1.2
          stamps=[10,21];i=2;undo();near(a.currentTime,(21-1.2-4)/1.2);
          stamps=[10,21,22];i=3;redoLine();assert.equal(i,2);near(a.currentTime,14);
          prevLine();assert.equal(i,0);near(a.currentTime,4);
          stamps=[10];i=1;restart();near(a.currentTime,4);assert.equal(a.paused,true);
          seekSong(-10);assert.equal(a.currentTime,0);
        """)

    def test_autosave_restores_next_offset_and_reaction(self):
        self.js("""
          assert.equal(i,2);assert.equal(reaction,0.2);assert.equal(a.currentTime,14);
          assert.equal(a.paused,true);el('start').click();assert.equal(a.currentTime,14);
          a.currentTime=15;stamp();assert.ok(Math.abs(stamps[2]-21.808)<1e-9);
          const saved=JSON.parse(storage.get(KEY));assert.equal(saved.reaction,0.2);
          assert.equal(saved.stamps.length,3);
        """, "storage.set('tap-song-v2-dump',JSON.stringify({stamps:[10,21],reaction:0.2}));")

    def test_old_audio_clock_arrays_are_ignored(self):
        self.js("assert.equal(i,0);assert.equal(stamps.length,0);assert.notEqual(KEY,'tap-dump');",
                "storage.set('tap-dump','[8,9]');storage.set('tap-song-v2-dump','[8,9]');")

    def test_file_resume_requires_song_header_and_seeks(self):
        self.js("""
          el('load').onchange({target:{files:['8\\t8\\tone\\n']}});assert.equal(i,0);
          el('load').onchange({target:{files:['# clock: song\\n# note\\n10\\t10\\tone\\n21\\t21\\ttwo\\n']}});
          assert.equal(i,2);assert.equal(a.currentTime,14);assert.equal(a.paused,true);
        """)

    def test_calibration_pauses_blocks_repeats_and_saves(self):
        self.js("""
          a.paused=false;const pending=el('cal').click();assert.equal(a.paused,true);
          key('KeyP');assert.equal(a.paused,true);
          el('back').click();assert.equal(a.paused,true);
          for(let k=0;k<8;k++){
            ctx.currentTime=0.55+k;key('Space',true);
            ctx.currentTime=0.7+k;key('Space');
          }
          assert.equal(i,0);timers.shift()();await pending;
          assert.ok(Math.abs(reaction-0.2)<1e-9);assert.equal(ctx.closed,true);
          assert.ok(Math.abs(JSON.parse(storage.get(KEY)).reaction-0.2)<1e-9);
          a.paused=false;a.currentTime=10;key('Space',true);assert.equal(i,0);
          key('Space');assert.equal(i,1);
        """)

    def test_legacy_line_mapping_uses_text_not_candidate_times(self):
        self.js("assert.deepEqual(TOK.map(t=>t.line),[0,0,1,1]);")

    def test_legacy_text_mismatch_rejected(self):
        self.write('labels.template.txt', '10\t10\twrong\n')
        self.assertIn('legacy template tokens do not match', self.generate(False))

    def test_legacy_syllables_and_punctuation(self):
        self.write('lyrics.lrc', '[00:10.00]한글 hi!\n[00:20.00]日本 go\n')
        words = ['한', '글', 'hi!', '日', '本', 'go']
        self.write('labels.template.txt', ''.join(f'30\t30\t{t}\n' for t in words))
        self.js("assert.deepEqual(TOK.map(t=>t.line),[0,0,0,1,1,1]);")

    def test_sidecar_mapping_and_script_escaping(self):
        evil = '</script><script>throw Error("injected")</script>&__TOK__'
        self.meta['title'] = evil
        self.write('meta.json', json.dumps(self.meta))
        self.write('labels.template.json', json.dumps([
            {'t': 21000, 'text': evil, 'line_t': 10000},
            {'t': 22000, 'text': 'three', 'line_t': 20000}]))
        page = self.generate()
        self.assertEqual(page.count('<script>'), 1)
        self.assertEqual(page.count('</script>'), 1)
        self.assertIn('&lt;/script&gt;', page)
        self.js('assert.equal(TOK[0].line,0);assert.equal(TOK[0].t,21);'
                + 'assert.equal(TOK[0].text,' + json.dumps(evil) + ');', page=page)

    def test_sidecar_indexes_disambiguate_duplicate_stamps_with_empty_markers(self):
        for prefix, first_index in [('', 0), ('[00:05.00]\n[00:10.00]   \n', 2)]:
            with self.subTest(empty_markers=bool(prefix)):
                self.write('lyrics.lrc', prefix + '[00:10.00]one two\n[00:10.00]three four\n')
                self.write('labels.template.json', json.dumps([
                    {'t': 11000, 'text': 'one', 'line_t': 10000, 'line_index': first_index},
                    {'t': 12000, 'text': 'three', 'line_t': 10000, 'line_index': first_index + 1}]))
                self.js(f'assert.deepEqual(TOK.map(t=>t.line),[{first_index},{first_index + 1}]);'
                        "assert.equal(LINES[TOK[0].line],'one two');"
                        "assert.equal(LINES[TOK[1].line],'three four');")

    def test_sidecar_index_must_be_valid_and_match_stamp(self):
        for index in [-1, 2, 0.5, True, None, '0', 1]:
            with self.subTest(index=index):
                self.write('labels.template.json', json.dumps([
                    {'t': 11000, 'text': 'one', 'line_t': 10000, 'line_index': index}]))
                self.assertIn('sidecar line_index', self.generate(False))

    def test_legacy_sidecar_unique_stamp_retains_empty_marker_index(self):
        self.write('lyrics.lrc', '[00:05.00]\n[00:10.00]one two\n')
        self.write('labels.template.json', json.dumps([
            {'t': 11000, 'text': 'one', 'line_t': 10000}]))
        self.js("assert.equal(TOK[0].line,1);assert.deepEqual(LINES,['','one two']);")

    def test_legacy_sidecar_ambiguous_stamp_requires_index(self):
        self.write('lyrics.lrc', '[00:10.00]one two\n[00:10.00]three four\n')
        self.write('labels.template.json', json.dumps([
            {'t': 11000, 'text': 'one', 'line_t': 10000}]))
        self.assertIn('sidecar line_t must identify exactly one lyric line', self.generate(False))

    def test_evidence_survives_source_eviction(self):
        expected = {name: (self.dump / name).read_bytes()
                    for name in ('meta.json', 'lyrics.lrc', 'pcm.i16', 'words.json')}
        page = self.generate()
        shutil.rmtree(self.dump)
        for name, data in expected.items():
            self.assertEqual((self.out / name).read_bytes(), data)
        self.js('assert.equal(TOK.length,4);', page=page)

    def test_sample_rate_controls_wav_and_clock(self):
        self.meta['sample_rate'] = 8000
        self.write('meta.json', json.dumps(self.meta))
        page = self.generate()
        with wave.open(str(self.out / 'song.wav')) as audio:
            self.assertEqual(audio.getframerate(),8000)
        self.js('assert.equal(songTime(10),10);seekSong(10);assert.equal(a.currentTime,10);', page=page)


if __name__ == '__main__':
    unittest.main()
