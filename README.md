# exifrensc
Renames image files using EXIF data, but maintains synchronisation with Nikon side car files as well.
## Rational
I had a bunch of .nef files which had been modified in either Capture NX-D or NX Studio, so consequently they had associated Nikon side car files. I wanted to bulk rename them using EXIF tags. Seems simple enough, the nef files would have been easy to rename but the associated side car files would have fallen out of sync, so I decided to create a program to rename the nef files and rename the side car files at the same time so the two remained linked together.

My secondary objective was to learn how to use the win32 API from Rust. This has proven a little more convoluted than I originally expected, but this program demonstrates using many different controls from within the win32 family so might help some people.
## Roadmap
### Done
- Create a main program window from a dialog saved in the resource stub
- Implement WM_INITDIALOG
- Set up WM_COMMAND
- Process WM_SIZE
- Process WM_DROPFILES
- Process CreateFontA
- Add and process tool tips
- Write a parser in build.rs to process the resource file made by ResEdit and add constants Rust can digest
- Create ListView
### To DO
- Add functions to process ListViews (e.g. adding entries, deleting entries)
- Add sqlite
- Code renaming logic
## License
Copyright © 2022 Andrew Roach. All rights reserved.

GNU General Public License version 3

exifrensc is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

exifrensc is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.