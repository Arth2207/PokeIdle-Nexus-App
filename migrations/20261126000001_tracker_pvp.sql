-- Última amostra guardada do Tracker, sem ordenar os demais eventos do jogo.
CREATE INDEX IF NOT EXISTS evento_tracker_captura_em
    ON nexus.evento (em DESC)
    WHERE tipo = 'tracker.captura';
