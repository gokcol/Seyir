# Seyir privacy policy

Effective 21 November 2026

Seyir is a desktop image viewer by Orhan Gökçöl. This policy covers the macOS and Windows editions, including the Microsoft Store edition.

## Images, metadata, and folders

Seyir reads files in the folders you browse and opens and processes images on your device. Image metadata may contain personal information. Seyir reads metadata to display image properties and writes changes to supported files when you choose to save them. Viewing, editing, and exporting images do not send your files or metadata to the developer. Seyir requires no app account and has no in-app advertising, analytics, or automatic crash-report uploads.

## Preferences and recent images

Seyir stores your preferences and paths for up to 12 recently opened images in per-user application data. File and folder names in those paths may contain personal information. On macOS, the files are in `~/Library/Application Support/io.github.gokcol.Seyir`. On Windows, they are normally in `%APPDATA%\gokcol\Seyir\config`. Windows may store this data in package-specific storage for the Microsoft Store edition.

The history is held in the `recent-images` file. You can view it through **File → Recent Images** or clear it by closing Seyir and removing that history file. Exported images stay in the folders you choose. Local storage is protected by your operating system permissions and any disk encryption you enable. If you choose a shared, network, or synced folder, access and syncing are governed by that service and your settings.

## Copying, printing, and opening in Bediz

If you copy an image, Seyir puts the image onto the system clipboard. Other apps and any enabled clipboard-sync service may be able to access it. Printing sends the image through the printing service and to the printer you choose.

If you choose **Edit in Bediz**, Seyir sends your selected image to a compatible, locally installed Bediz editor. Depending on the action, it passes the path of the original file, writes a rendered PNG in the system temporary folder, or copies the image to the clipboard and opens Bediz for manual paste. A rendered temporary copy can remain on disk until it is removed. Bediz handles images it receives under its own [privacy policy](https://github.com/gokcol/MyImage/blob/main/PRIVACY.md).

## Update checks and external links

On macOS, choosing **Check for Updates** contacts GitHub’s public release API. GitHub receives ordinary connection information, including your IP address and Seyir’s update-check user agent. A Microsoft Store installation of Seyir uses Microsoft’s Store service to check for updates to its package. Seyir does not include images, image metadata, or recent file paths in update requests.

External links open their destination in the browser or Microsoft Store. Those services process requests under their own privacy policies. Seyir’s update checker only runs when requested and lets you open the update source yourself.

## Contact

For privacy or support questions, contact [orhan.gokcol@hotmail.com](mailto:orhan.gokcol@hotmail.com). Please avoid including private images or personal details in public GitHub issues. Changes to this policy will be published on this page with an updated effective date.
