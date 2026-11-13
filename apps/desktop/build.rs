fn main() {
    // Comandos do app ficam atrás de permissões (`allow-<comando>`): a página remota do jogo
    // só recebe as três que o coletor usa (ver capabilities/jogo.json).
    let manifest = tauri_build::AppManifest::new().commands(&[
        "iniciar_nucleo",
        "salvar_token",
        "tem_token",
        "posicionar_conta",
        "pausar_conta",
        "recarregar_conta",
        "abrir_tracker",
        "coletor_evento",
        "coletor_resultado",
        "tracker_captura",
    ]);
    if let Err(e) = tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest)) {
        eprintln!("falha no tauri-build: {e}");
        std::process::exit(1);
    }
}
