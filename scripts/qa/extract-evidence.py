"""Extract Windows Compress-Archive output without importing Windows file attributes."""
import argparse
from pathlib import Path, PurePosixPath
import shutil
import zipfile


def extract(archive: Path, destination: Path):
    destination.mkdir(parents=True, exist_ok=True)
    root=destination.resolve()
    with zipfile.ZipFile(archive) as source:
        for member in source.infolist():
            name=member.filename.replace('\\','/')
            relative=PurePosixPath(name)
            if relative.is_absolute() or '..' in relative.parts:
                raise ValueError(f'Unsafe evidence path: {member.filename}')
            target=root.joinpath(*relative.parts)
            if not target.resolve().is_relative_to(root):
                raise ValueError(f'Evidence path escapes output: {member.filename}')
            if name.endswith('/'):
                target.mkdir(parents=True,exist_ok=True)
            else:
                target.parent.mkdir(parents=True,exist_ok=True)
                with source.open(member) as incoming, target.open('wb') as outgoing:
                    shutil.copyfileobj(incoming,outgoing)
            # Compress-Archive attributes are not POSIX permissions. In particular,
            # its directory entries can otherwise become untraversable mode 000.
            target.chmod(0o755 if target.is_dir() else 0o644)


if __name__=='__main__':
    p=argparse.ArgumentParser()
    p.add_argument('archive',type=Path)
    p.add_argument('destination',type=Path)
    a=p.parse_args()
    extract(a.archive,a.destination)
