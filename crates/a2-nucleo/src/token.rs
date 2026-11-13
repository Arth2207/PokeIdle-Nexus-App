//! Token do usuário guardado em arquivo no diretório de configuração, com permissão restrita
//! (0600 em Unix; no Windows o perfil do usuário já é privado). A trait permite trocar por
//! keyring/credential manager depois.

use crate::Erro;
use std::path::{Path, PathBuf};

pub trait ArmazemToken: Send + Sync {
    fn ler(&self) -> Result<Option<String>, Erro>;
    fn gravar(&self, token: &str) -> Result<(), Erro>;
    fn apagar(&self) -> Result<(), Erro>;
}

#[derive(Debug, Clone)]
pub struct ArquivoToken {
    caminho: PathBuf,
}

impl ArquivoToken {
    pub fn em(caminho: impl Into<PathBuf>) -> Self {
        Self {
            caminho: caminho.into(),
        }
    }

    /// `<config do usuário>/<app>/token`: `APPDATA` (Windows), `XDG_CONFIG_HOME` ou `~/.config`.
    pub fn padrao(app: &str) -> Result<Self, Erro> {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let base = var("APPDATA")
            .or_else(|| var("XDG_CONFIG_HOME"))
            .or_else(|| var("HOME").map(|h| h.join(".config")))
            .ok_or_else(|| Erro::Token("sem diretório de configuração".into()))?;
        Ok(Self::em(base.join(app).join("token")))
    }

    pub fn caminho(&self) -> &Path {
        &self.caminho
    }
}

fn io(e: std::io::Error) -> Erro {
    Erro::Token(e.to_string())
}

impl ArmazemToken for ArquivoToken {
    fn ler(&self) -> Result<Option<String>, Erro> {
        match std::fs::read_to_string(&self.caminho) {
            Ok(s) => {
                let t = s.trim().to_string();
                Ok(if t.is_empty() { None } else { Some(t) })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(e)),
        }
    }

    fn gravar(&self, token: &str) -> Result<(), Erro> {
        use std::io::Write;
        if let Some(dir) = self.caminho.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        // Grava num temporário já restrito e renomeia: nunca existe cópia legível por outros.
        let tmp = self.caminho.with_extension("tmp");
        let mut op = std::fs::OpenOptions::new();
        op.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut op, 0o600);
        let mut f = op.open(&tmp).map_err(io)?;
        f.write_all(token.trim().as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
        std::fs::rename(&tmp, &self.caminho).map_err(io)
    }

    fn apagar(&self) -> Result<(), Erro> {
        match std::fs::remove_file(&self.caminho) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn temp(nome: &str) -> ArquivoToken {
        let d = std::env::temp_dir().join(format!("a2-token-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        ArquivoToken::em(d.join("sub").join("token"))
    }

    #[test]
    fn ausente_gravar_ler_apagar() {
        let a = temp("ciclo");
        assert_eq!(a.ler().ok(), Some(None));
        assert!(a.gravar("  segredo\n").is_ok());
        assert_eq!(a.ler().ok(), Some(Some("segredo".to_string())));
        assert!(a.gravar("novo").is_ok());
        assert_eq!(a.ler().ok(), Some(Some("novo".to_string())));
        assert!(a.apagar().is_ok());
        assert!(a.apagar().is_ok());
        assert_eq!(a.ler().ok(), Some(None));
    }

    #[cfg(unix)]
    #[test]
    fn permissao_e_0600() {
        use std::os::unix::fs::PermissionsExt;
        let a = temp("perm");
        assert!(a.gravar("x").is_ok());
        let modo = std::fs::metadata(a.caminho()).map(|m| m.permissions().mode() & 0o777);
        assert_eq!(modo.ok(), Some(0o600));
    }

    #[test]
    fn arquivo_vazio_conta_como_sem_token() {
        let a = temp("vazio");
        assert!(a.gravar("   ").is_ok());
        assert_eq!(a.ler().ok(), Some(None));
    }
}
