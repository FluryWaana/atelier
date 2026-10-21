import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

test('launching after installation does not pass RedirectionGuard to the application', { skip: process.platform !== 'win32', timeout: 30000 }, async t => {
  const processes = execFileSync('tasklist.exe', ['/FI', 'IMAGENAME eq explorer.exe', '/NH'], { encoding: 'utf8', windowsHide: true });
  if (!/explorer\.exe/i.test(processes)) { t.skip('An interactive Windows desktop is required.'); return; }
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'atelier-launch-')));
  t.after(() => {
    assert.ok(resolve(root).startsWith(realpathSync(tmpdir()) + sep));
    rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  });
  mkdirSync(join(root, 'target'));
  writeFileSync(join(root, 'target', 'probe.txt'), 'ok');
  symlinkSync(join(root, 'target'), join(root, 'linked'), 'junction');
  const source = join(root, 'probe.cs'), binary = join(root, 'AtelierKanban.exe');
  writeFileSync(source, `using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
class Probe {
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetProcessMitigationPolicy(int policy, ref uint flags, UIntPtr size);
  static void Main(string[] args) {
    string root=AppDomain.CurrentDomain.BaseDirectory, result;
    if(args.Length==2) {
      uint flags=1;
      if(!SetProcessMitigationPolicy(16,ref flags,new UIntPtr(4))) { File.WriteAllText(Path.Combine(root,"control"),"unsupported"); return; }
    }
    try { result=File.ReadAllText(Path.Combine(root,"linked","probe.txt")); }
    catch(Exception error) { result="error:"+(error.HResult & 65535); }
    File.WriteAllText(Path.Combine(root,args.Length==2 ? "control" : "result"),result);
    if(args.Length==2 && result=="error:448") {
      using(var process=Process.Start(new ProcessStartInfo(args[0],args[1]) { UseShellExecute=false, CreateNoWindow=true })) {
        if(!process.WaitForExit(15000)) throw new Exception("Launch timed out.");
      }
    }
  }
}`);
  const compiler = join(process.env.SystemRoot, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe');
  execFileSync(compiler, ['/nologo', '/target:winexe', '/out:' + binary, source], { windowsHide: true });
  const template = readFileSync(new URL('../scripts/installers/windows.iss', import.meta.url), 'utf8');
  const entry = template.split('[Run]')[1].split('[Code]')[0].split(/\r?\n/).find(line => line.startsWith('Filename:'));
  const field = name => (entry.match(new RegExp(name + ': "((?:""|[^"])*)"'))?.[1] || '').replaceAll('""', '"').replaceAll('{app}', root).replaceAll('{win}', process.env.SystemRoot);
  execFileSync(binary, [field('Filename'), field('Parameters')], { windowsHide: true, timeout: 20000 });
  const control = readFileSync(join(root, 'control'), 'utf8');
  if (control === 'unsupported' || control === 'ok') { t.skip('This Windows account does not enforce RedirectionGuard on the test junction.'); return; }
  assert.equal(control, 'error:448', 'The installer context must refuse the user-created junction.');
  for (let i = 0; i < 100 && !existsSync(join(root, 'result')); i++) await delay(100);
  assert.equal(readFileSync(join(root, 'result'), 'utf8'), 'ok', 'The application must be launched in the normal desktop context.');
});
