//! Compressão das mensagens do WebSocket com o subprotocolo `nexus.v1.deflate`.
//!
//! O `tungstenite` não tem `permessage-deflate`, então a aplicação comprime sozinha: o payload
//! é o JSON do envelope em deflate cru (RFC 1951, sem cabeçalho zlib), dentro de um frame
//! binário. Usa só `flate2` com o backend em Rust puro, que também compila para WebAssembly.

use std::io::{Read, Write};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;

/// Mensagens do servidor acima deste tamanho (bytes de JSON) vão comprimidas.
pub const LIMITE_COMPRESSAO: usize = 1024;

/// Teto do JSON descomprimido aceito (o mesmo limite de corpo da ingestão HTTP).
pub const LIMITE_DESCOMPRIMIDO: usize = 10 * 1024 * 1024;

/// Por que uma descompressão falhou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErroCompressao {
    /// O resultado passaria do limite (possível zip bomb).
    ExcedeLimite,
    /// Dados que não são deflate válido ou não são UTF-8.
    Invalido,
}

/// Comprime o texto em deflate cru. `None` só se o compressor falhar (não acontece em memória).
pub fn comprimir(texto: &str) -> Option<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(texto.as_bytes()).ok()?;
    enc.finish().ok()
}

/// Descomprime um frame binário em texto, lendo no máximo `limite` bytes.
pub fn descomprimir(dados: &[u8], limite: usize) -> Result<String, ErroCompressao> {
    let mut saida = Vec::new();
    // Lê um byte a mais que o limite: se ele vier, o conteúdo passou do teto.
    let max = u64::try_from(limite).unwrap_or(u64::MAX).saturating_add(1);
    DeflateDecoder::new(dados)
        .take(max)
        .read_to_end(&mut saida)
        .map_err(|_| ErroCompressao::Invalido)?;
    if saida.len() > limite {
        return Err(ErroCompressao::ExcedeLimite);
    }
    String::from_utf8(saida).map_err(|_| ErroCompressao::Invalido)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ida_e_volta() {
        let t = "abc".repeat(2000);
        let c = comprimir(&t).unwrap_or_default();
        assert!(c.len() < t.len());
        assert_eq!(descomprimir(&c, LIMITE_DESCOMPRIMIDO).as_deref(), Ok(&*t));
    }

    #[test]
    fn rejeita_bomba_e_lixo() {
        let t = "0".repeat(LIMITE_DESCOMPRIMIDO + 1);
        let c = comprimir(&t).unwrap_or_default();
        assert_eq!(
            descomprimir(&c, LIMITE_DESCOMPRIMIDO),
            Err(ErroCompressao::ExcedeLimite)
        );
        assert_eq!(
            descomprimir(&[0xff, 0xff, 0xff], 100),
            Err(ErroCompressao::Invalido)
        );
    }
}
