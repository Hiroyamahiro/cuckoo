param([switch]$TestsOnly)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$build=Join-Path $PSScriptRoot 'build';$dist=Join-Path $PSScriptRoot 'dist'
New-Item -ItemType Directory -Force -Path $build,$dist | Out-Null
$csc=Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if(-not(Test-Path -LiteralPath $csc)){throw '.NET Framework compiler not found'}
$package=Get-Content (Join-Path $root 'package.json') -Raw -Encoding UTF8|ConvertFrom-Json
$version=[version]$package.version
$assembly=Join-Path $build 'AssemblyInfo.cs'
[IO.File]::WriteAllText($assembly,('[assembly:System.Reflection.AssemblyVersion("'+$version.ToString(3)+'.0")][assembly:System.Reflection.AssemblyFileVersion("'+$version.ToString(3)+'.0")][assembly:System.Reflection.AssemblyProduct("Cuckoo")][assembly:System.Reflection.AssemblyCompany("ひろやまひろし / Hiroyama Hiroshi")]'),[Text.UTF8Encoding]::new($false))
$common=@('/nologo','/optimize+','/platform:x64','/r:System.Windows.Forms.dll','/r:System.Drawing.dll','/r:System.Runtime.Serialization.dll','/r:System.Core.dll',('/win32manifest:'+(Join-Path $PSScriptRoot 'Setup.manifest')),('/win32icon:'+(Join-Path $root 'assets\icons\app\cuckoo-app.ico')),('/resource:'+(Join-Path $root 'assets\icons\app\cuckoo-app.ico')+',Cuckoo.Icon'),('/resource:'+(Join-Path $root 'assets\icons\app\cuckoo-app-256.png')+',Cuckoo.Logo'))
$core=@((Join-Path $PSScriptRoot 'Update.cs'),(Join-Path $PSScriptRoot 'Backend.cs'),(Join-Path $PSScriptRoot 'Flow.cs'),(Join-Path $PSScriptRoot 'SetupForm.cs'),$assembly)
if($TestsOnly){& $csc @common '/target:exe' ('/out:'+(Join-Path $build 'SetupTests.exe')) @core (Join-Path $PSScriptRoot 'tests\SetupTests.cs');if($LASTEXITCODE){throw 'Tests compilation failed'};& (Join-Path $build 'SetupTests.exe');if($LASTEXITCODE){throw 'Setup tests failed'};& $csc @common '/target:winexe' '/define:SETUP_TEST' ('/out:'+(Join-Path $build 'SetupPreview.exe')) @core (Join-Path $PSScriptRoot 'tests\Preview.cs');if($LASTEXITCODE){throw 'Preview compilation failed'};exit 0}
$maintenance=Join-Path $build 'CuckooMaintenance.exe'
& $csc @common '/target:winexe' '/define:MAINTENANCE' ('/out:'+$maintenance) @core (Join-Path $PSScriptRoot 'Program.cs')
if($LASTEXITCODE){throw 'Maintenance compilation failed'}
Push-Location $root
try{& npm.cmd run tauri -- build;if($LASTEXITCODE){throw 'Tauri engine build failed'}}finally{Pop-Location}
$internal=Join-Path $root ('src-tauri\target\release\bundle\nsis\Cuckoo_'+$package.version+'_x64-setup.exe')
$wv=Join-Path $build 'MicrosoftEdgeWebview2Setup.exe'
if(-not(Test-Path $wv)){Invoke-WebRequest 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $wv -UseBasicParsing}
$signature=Get-AuthenticodeSignature -LiteralPath $wv
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation'){throw 'Microsoft WebView2 bootstrapper signature invalid'}
function Record($file,$relative){[ordered]@{Path=$relative;Sha256=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant();Size=(Get-Item -LiteralPath $file).Length}}
# Tauri restores the build EXE's UNK marker after bundling; NSIS stores NSS.
# Compute the packaged image digest with the same fixed-width marker patch.
$appFile=Join-Path $root 'src-tauri\target\release\Cuckoo.exe'
$appBytes=[IO.File]::ReadAllBytes($appFile)
$marker='TAURI_BUNDLE_TYPE_VAR_UNK'
$appText=[Text.Encoding]::ASCII.GetString($appBytes)
$markerIndex=$appText.IndexOf($marker,[StringComparison]::Ordinal)
if($markerIndex -lt 0 -or $markerIndex -ne $appText.LastIndexOf($marker,[StringComparison]::Ordinal)){throw 'Unexpected Tauri bundle marker; review packaging contract'}
[Array]::Copy([Text.Encoding]::ASCII.GetBytes('NSS'),0,$appBytes,$markerIndex+$marker.Length-3,3)
$algorithm=[Security.Cryptography.SHA256]::Create()
try{$appHash=[BitConverter]::ToString($algorithm.ComputeHash($appBytes)).Replace('-','').ToLowerInvariant()}finally{$algorithm.Dispose()}
$appRecord=[ordered]@{Path='Cuckoo.exe';Sha256=$appHash;Size=$appBytes.LongLength}
$installed=@($appRecord,(Record $maintenance 'CuckooMaintenance.exe'),(Record (Join-Path $root 'docs\USER-GUIDE-ja.md') 'README-ja.md'))
$manifest=[ordered]@{Version=$package.version;Engine=(Record $internal 'engine.exe');WebView=(Record $wv 'webview2.exe');Installed=$installed}
$manifestFile=Join-Path $build 'payload-manifest.json';[IO.File]::WriteAllText($manifestFile,($manifest|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
$output=Join-Path $dist ('Cuckoo_'+$package.version+'_x64-setup.exe')
& $csc @common '/target:winexe' ('/out:'+$output) ('/resource:'+$manifestFile+',Cuckoo.Manifest') ('/resource:'+$internal+',Cuckoo.Engine') ('/resource:'+$wv+',Cuckoo.WebView') @core (Join-Path $PSScriptRoot 'Program.cs')
if($LASTEXITCODE){throw 'Setup compilation failed'}
$hash=(Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS.txt'),($hash+'  '+[IO.Path]::GetFileName($output)+"`n"),[Text.UTF8Encoding]::new($false))
Write-Output ('Distribution installer: '+$output)
Write-Output ('SHA256: '+$hash)
