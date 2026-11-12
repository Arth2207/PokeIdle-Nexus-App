-- Fila de comandos: idempotência do inserir, agendamento e avisos de estado. Só aditiva.

-- Chave de idempotência do `POST /api/inserir-comando` (header `Idempotency-Key` ou campo
-- `chave`). Única por conta; a janela de 24 h é aplicada na aplicação (chave velha é limpa).
ALTER TABLE nexus.comando ADD COLUMN chave_idem text;
CREATE UNIQUE INDEX comando_chave_idem ON nexus.comando (conta, chave_idem)
    WHERE chave_idem IS NOT NULL;

-- Agendamento (`NEXUS_MELHORIAS`): a reserva ignora o comando até `executar_em`. Agendado nasce
-- com `agendado_avisado = false`; quando a hora chega a manutenção marca `true`, e o trigger
-- avisa como comando novo.
ALTER TABLE nexus.comando ADD COLUMN executar_em timestamptz;
ALTER TABLE nexus.comando ADD COLUMN agendado_avisado boolean NOT NULL DEFAULT true;

-- Dead-letter: encerrado sem confirmação (tentativas esgotadas).
CREATE INDEX comando_falho ON nexus.comando (conta, id)
    WHERE executado_em IS NOT NULL AND resultado = 'erro:sem confirmacao';

-- O aviso passa a cobrir também reserva, execução, reenvio e liberação de agendado. Quem já
-- escuta ignora `reservado` e `executado`.
CREATE OR REPLACE FUNCTION nexus.comando_notificar() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    evento text;
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- Encerrado na criação ou ainda agendado para depois: sem aviso.
        IF NEW.executado_em IS NOT NULL OR NOT NEW.agendado_avisado THEN
            RETURN NULL;
        END IF;
        evento := 'novo';
    ELSIF NEW.executado_em IS NOT NULL AND OLD.executado_em IS NULL THEN
        IF NEW.resultado IS NOT DISTINCT FROM 'expirado' THEN
            evento := 'expirado';
        ELSIF NEW.resultado IS DISTINCT FROM 'erro:sem confirmacao' THEN
            evento := 'executado';
        ELSE
            RETURN NULL;
        END IF;
    ELSIF NEW.executado_em IS NULL AND OLD.executado_em IS NOT NULL THEN
        -- Reenvio de um comando falho: volta a pendente.
        evento := 'novo';
    ELSIF NEW.executado_em IS NULL AND NEW.reservado_em IS NULL AND NEW.agendado_avisado
          AND NOT OLD.agendado_avisado THEN
        evento := 'novo';
    ELSIF NEW.reservado_em IS NULL AND OLD.reservado_em IS NOT NULL AND NEW.executado_em IS NULL THEN
        evento := 'devolvido';
    ELSIF NEW.reservado_em IS NOT NULL AND OLD.reservado_em IS NULL AND NEW.executado_em IS NULL THEN
        evento := 'reservado';
    ELSE
        RETURN NULL;
    END IF;
    PERFORM pg_notify('nexus_comando',
        json_build_object('id', NEW.id, 'conta', NEW.conta, 'evento', evento)::text);
    RETURN NULL;
END
$$;
