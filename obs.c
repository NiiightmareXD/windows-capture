static bool GetMonitorTarget(LPCWSTR device,
			     DISPLAYCONFIG_TARGET_DEVICE_NAME *target)
{
	bool found = false;

	UINT32 numPath, numMode;
	if (GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &numPath,
					&numMode) == ERROR_SUCCESS) {
		DISPLAYCONFIG_PATH_INFO *paths =
			bmalloc(numPath * sizeof(DISPLAYCONFIG_PATH_INFO));
		DISPLAYCONFIG_MODE_INFO *modes =
			bmalloc(numMode * sizeof(DISPLAYCONFIG_MODE_INFO));
		if (QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &numPath, paths,
				       &numMode, modes,
				       NULL) == ERROR_SUCCESS) {
			for (size_t i = 0; i < numPath; ++i) {
				const DISPLAYCONFIG_PATH_INFO *const path =
					&paths[i];

				DISPLAYCONFIG_SOURCE_DEVICE_NAME
				source;
				source.header.type =
					DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
				source.header.size = sizeof(source);
				source.header.adapterId =
					path->sourceInfo.adapterId;
				source.header.id = path->sourceInfo.id;
				if (DisplayConfigGetDeviceInfo(
					    &source.header) == ERROR_SUCCESS &&
				    wcscmp(device, source.viewGdiDeviceName) ==
					    0) {
					target->header.type =
						DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
					target->header.size = sizeof(*target);
					target->header.adapterId =
						path->sourceInfo.adapterId;
					target->header.id = path->targetInfo.id;
					found = DisplayConfigGetDeviceInfo(
							&target->header) ==
						ERROR_SUCCESS;
					break;
				}
			}
		}

		bfree(modes);
		bfree(paths);
	}

	return found;
}

static void GetMonitorName(HMONITOR handle, char *name, size_t count)
{
	MONITORINFOEXW mi;
	DISPLAYCONFIG_TARGET_DEVICE_NAME target;

	mi.cbSize = sizeof(mi);
	if (GetMonitorInfoW(handle, (LPMONITORINFO)&mi) &&
	    GetMonitorTarget(mi.szDevice, &target)) {
		char *friendly_name;
		os_wcs_to_utf8_ptr(target.monitorFriendlyDeviceName, 0,
				   &friendly_name);

		strcpy_s(name, count, friendly_name);
		bfree(friendly_name);
	} else {
		strcpy_s(name, count, "[OBS: Unknown]");
	}
}