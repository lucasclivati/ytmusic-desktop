//! Embute o icone (e metadados basicos) nos executaveis do Windows, para que o
//! Explorer, o menu Iniciar, a barra de tarefas e o instalador mostrem o logo.
//! O arquivo gerado por tools/make-icon.ps1 fica em assets/icon.ico.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    res.set("ProductName", "YT Music");
    res.set("FileDescription", "YT Music");
    // Falha alto de proposito: um instalador sem icone passaria despercebido.
    // O rc.exe vem com o Windows SDK, que ja e necessario para linkar no Windows.
    res.compile().expect("falha ao embutir o icone (rc.exe do Windows SDK nao encontrado?)");
}
