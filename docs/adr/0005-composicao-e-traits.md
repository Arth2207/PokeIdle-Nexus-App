# 0005. Composição e traits no lugar de herança

- Status: aceito
- Data: 2026-11-11

## Contexto

Rust não tem classes nem herança. Ele tem `struct` (dados), `impl` (métodos), `trait` (contratos de comportamento) e visibilidade controlada por módulo. A dúvida era como organizar um domínio relativamente grande (bolsa, combate, automação, scoring) sem as ferramentas tradicionais de orientação a objetos.

## Decisão

- **Objetos:** `struct` com campos privados e métodos em `impl`. O encapsulamento é garantido pelo compilador.
- **Interfaces:** `trait`. Todo acesso a algo externo (banco, relógio, gerador aleatório) é feito por um trait definido no `nucleo`.
- **Reaproveitamento:** composição. Uma struct contém outras, em vez de herdar delas.
- **Arquitetura hexagonal:** o `nucleo` define os traits (portas) e o crate `persistencia` fornece as implementações (adaptadores).

```rust
pub trait Relogio {
    fn agora_ms(&self) -> i64;
}

pub struct AvaliadorBolsa<R: RepoMercado, C: Relogio> {
    repo: R,
    relogio: C,
}
```

## Consequências

- As regras de negócio são testadas sem banco e sem tempo real: basta passar um repositório em memória e um relógio fixo.
- Trocar a implementação de um repositório não toca no domínio.
- Não existem as hierarquias rígidas da herança, que são difíceis de mudar depois.
- Custo: para quem vem de OO tradicional, genéricos com trait bounds levam um tempo para ficarem naturais. A documentação do código explica cada um quando aparece pela primeira vez.
