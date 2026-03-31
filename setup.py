from setuptools import setup, find_packages

setup(
    name="kryos",
    version="0.1.0",
    description="Kryos Programming Language - Universal systems language with native AI, quantum, and security capabilities",
    author="FrostByte Digital",
    packages=find_packages(),
    entry_points={
        "console_scripts": [
            "kryos=kryos.cli:main",
        ],
    },
    python_requires=">=3.10",
)
