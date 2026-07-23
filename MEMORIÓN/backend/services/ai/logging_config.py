from __future__ import annotations

import logging
import sys
from pathlib import Path

from platformdirs import user_data_path


def application_data_directory(app_name: str = "MEMORIÓN") -> Path:
    return user_data_path(app_name, appauthor=False)


def configure_model_logging(app_name: str = "MEMORIÓN") -> tuple[logging.Logger, Path]:
    log_directory = application_data_directory(app_name) / "logs"
    log_directory.mkdir(parents=True, exist_ok=True)
    log_path = log_directory / "model-download.log"

    logger = logging.getLogger("memorion.models")
    logger.setLevel(logging.INFO)
    logger.propagate = False
    if not logger.handlers:
        formatter = logging.Formatter(
            "%(asctime)s | %(levelname)s | %(message)s", datefmt="%Y-%m-%d %H:%M:%S"
        )
        console = logging.StreamHandler(sys.stdout)
        console.setFormatter(formatter)
        file_handler = logging.FileHandler(log_path, encoding="utf-8")
        file_handler.setFormatter(formatter)
        logger.addHandler(console)
        logger.addHandler(file_handler)
    return logger, log_path
