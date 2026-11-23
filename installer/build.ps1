# Gera o instalador: compila o release e empacota com o Inno Setup.
# Uso (na raiz do repositorio):  powershell -File installer\build.ps1
# Saida: installer\Output\YTMusic-Setup-<versao>.exe

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# Versao = a do Cargo.toml.
$version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { throw 'Nao achei a versao no Cargo.toml.' }

# Localiza o compilador do Inno Setup.
$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup nao encontrado. Instale: winget install JRSoftware.InnoSetup' }

if (Get-Process ytmusic, ytmusic-login -ErrorAction SilentlyContinue) {
    throw 'Feche o YT Music antes: o .exe em uso impede o build.'
}

# Privacidade: o rustc embute caminhos absolutos no binario (ex.: C:\Users\<voce>\.cargo\...),
# o que publicaria o seu usuario do Windows. Reescrevemos esses prefixos (calculados aqui,
# nada fica gravado no repositorio). CARGO_ENCODED_RUSTFLAGS separa os flags por 0x1f.
$cargoHome  = if ($env:CARGO_HOME)  { $env:CARGO_HOME }  else { Join-Path $env:USERPROFILE '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$flags = @(
    "--remap-path-prefix=$cargoHome=/cargo",
    "--remap-path-prefix=$rustupHome=/rustup",
    "--remap-path-prefix=$root=/src",
    "--remap-path-prefix=$env:USERPROFILE=/home"
)
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1f
$triple = 'x86_64-pc-windows-msvc'
$bin = "$root\target\$triple\release"

Push-Location $root
try {
    # O antivirus as vezes trava um build script recem-criado ("Acesso negado" / "arquivo em uso").
    # O progresso fica em cache, entao repetir costuma resolver.
    # --target explicito: o RUSTFLAGS (remap) so vale para o app e as dependencias, e os build
    # scripts ja compilados continuam em cache (evita recompila-los e atrair o antivirus).
    foreach ($attempt in 1..3) {
        cargo build --release --target $triple
        if ($LASTEXITCODE -eq 0) { break }
        if ($attempt -eq 3) { throw 'cargo build falhou.' }
        Write-Warning "cargo build falhou (tentativa $attempt de 3); tentando de novo..."
        Start-Sleep -Seconds 5
    }
} finally { Pop-Location }

# Trava de seguranca: nenhum executavel pode conter dados da sua maquina. Procura, em ASCII e em
# UTF-16 (como o Windows guarda strings), o caminho do usuario, o da pasta do projeto e o nome do PC.
# A mensagem de erro diz o TIPO do dado, nunca o valor.
function Assert-NoPersonalData([string]$Path) {
    # 28591 = ISO-8859-1 (1 byte = 1 char). [Encoding]::Latin1 nao existe no PowerShell 5.1.
    $text = [System.Text.Encoding]::GetEncoding(28591).GetString([System.IO.File]::ReadAllBytes($Path))
    $checks = [ordered]@{
        'pasta do usuario (Users\<usuario>)' = @("Users\$env:USERNAME", "Users/$env:USERNAME")
        'perfil do Windows'                  = @($env:USERPROFILE)
        'pasta do projeto'                   = @($root, $root.Replace('\', '/'))
    }
    if ($env:COMPUTERNAME.Length -ge 6) { $checks['nome do computador'] = @($env:COMPUTERNAME) }
    foreach ($kind in $checks.Keys) {
        foreach ($n in $checks[$kind]) {
            if ([string]::IsNullOrWhiteSpace($n)) { continue }
            $wide = (($n.ToCharArray() | ForEach-Object { "$_`0" }) -join '')
            foreach ($form in $n, $wide) {
                if ($text.IndexOf($form, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
                    throw "$(Split-Path -Leaf $Path) contem dados pessoais ($kind). Abortando; nada foi publicado."
                }
            }
        }
    }
}
foreach ($exe in 'ytmusic.exe', 'ytmusic-login.exe') { Assert-NoPersonalData "$bin\$exe" }

& $iscc "/DAppVersion=$version" "/DBinDir=$bin" "$PSScriptRoot\ytmusic.iss"
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup falhou.' }
Assert-NoPersonalData "$PSScriptRoot\Output\YTMusic-Setup-$version.exe"

$out = Get-Item "$PSScriptRoot\Output\YTMusic-Setup-$version.exe"
'{0}  ({1:N1} MB)' -f $out.FullName, ($out.Length / 1MB)
