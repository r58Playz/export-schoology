# export-schoology
Export your Schoology data via the API.

This program only does the bare minimum of parsing required to extract the data. Most of the files are directly from Schoology API. There should be enough data exported to make a Schoology-like UI, however.

## Usage
Create a file with:
```
schooldomain.schoology.com
3-legged client key
3-legged client token
optionally: 3-legged user key to skip the oauth process
optionally: 3-legged user token to skip the oauth process
```

Pass that file to the executable:
```
cargo r -- path/to/file
```

The executable will create a directory in the format `export_<timestamp>` in the current dir.
