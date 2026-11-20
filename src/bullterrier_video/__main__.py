"""`python -m bullterrier_video`: the same command line as the `bt-video` program."""

import sys

from .cli import main

sys.exit(main())
