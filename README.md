# export-schoology
Export your Schoology data via the API.

This program only does the bare minimum of parsing required to extract the data. Most of the files are directly from Schoology API. There should be enough data exported to make a Schoology-like UI, however.

## Usage
Create a file with:
```
schooldomain.schoology.com
3-legged client key
3-legged client token
comma-separated list of course IDs to also export (or a blank line; this is useful to also export archived courses)
optionally: 3-legged user key to skip the oauth process
optionally: 3-legged user token to skip the oauth process
```

Pass that file to the executable:
```
cargo r -- path/to/file
```

The executable will create a directory in the format `export_<timestamp>` in the current dir.

## Export archived courses
Run this on `schooldomain.schoology.com/courses/mycourses/past`:
```js
Array.from(document.querySelectorAll(".student-section"), (node)=>+node.firstChild.href.split("/")[4]).join(",")
```
This will give you a list of course IDs which you can add to the file.

I haven't figured out how to get this automatically from the API. If anyone knows how, please make a PR!
