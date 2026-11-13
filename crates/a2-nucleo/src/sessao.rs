//! Controle do `seq` do servidor: o cliente guarda o último processado e o manda em `retomar`.

use contrato::ws::Retomar;

#[derive(Debug, Default, Clone)]
pub struct Sessao {
    ultimo_seq: u64,
    /// O servidor pediu recarga completa (`sessao.ressincronizar`); a casca refaz o estado por HTTP.
    pub precisa_ressincronizar: bool,
}

impl Sessao {
    pub fn ultimo_seq(&self) -> u64 {
        self.ultimo_seq
    }

    /// Mensagem `retomar` para enviar logo após conectar.
    pub fn retomar(&self) -> Retomar {
        Retomar {
            desde: self.ultimo_seq,
        }
    }

    /// Registra o `seq` de uma mensagem. `false` = já vista (reentrega após reconexão): ignorar.
    /// Mensagem sem `seq` sempre vale.
    pub fn registrar(&mut self, seq: Option<u64>) -> bool {
        match seq {
            None => true,
            Some(s) if s > self.ultimo_seq => {
                self.ultimo_seq = s;
                true
            }
            Some(_) => false,
        }
    }

    /// `sessao.pronta`: numa instalação nova adota o `seq` atual do servidor (nada a recuperar).
    pub fn adotar_pronta(&mut self, seq_servidor: u64) {
        if self.ultimo_seq == 0 {
            self.ultimo_seq = seq_servidor;
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn seq_repetido_ou_antigo_e_ignorado() {
        let mut s = Sessao::default();
        assert!(s.registrar(Some(1)));
        assert!(s.registrar(Some(3)));
        assert!(!s.registrar(Some(3)));
        assert!(!s.registrar(Some(2)));
        assert!(s.registrar(None));
        assert_eq!(s.retomar().desde, 3);
    }

    #[test]
    fn pronta_so_adota_em_sessao_nova() {
        let mut s = Sessao::default();
        s.adotar_pronta(40);
        assert_eq!(s.ultimo_seq(), 40);
        s.adotar_pronta(99);
        assert_eq!(s.ultimo_seq(), 40);
    }
}
